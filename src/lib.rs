//! BN_Vol - 币安波动率与趋势监控系统
//!
//! 本项目实时监控 BTC/USDT 的：
//! 1. **瞬时波动率**: 基于 aggTrade 计算年化波动率
//! 2. **趋势信号**: 基于 VWAP 拟合 + OFI 判断价格趋势
//!
//! # 数据流
//! ```text
//! Binance WebSocket
//!     ├── aggTrade ──> 波动率计算 ──> 趋势拟合 ──> Telemetry 推送
//!     └── depth20@100ms ──> OFI 计算 (辅助趋势判断)
//! ```
//!
//! # 输出
//! - Telemetry WebSocket (端口 9001): 实时价差调整信号
//! - Slack 通知: 波动率直方图报告
//! - 日志: 详细运行状态

pub mod common;
pub mod indicators;
pub mod config;
pub mod stats;
pub mod models;
pub mod notifier;
pub mod telemetry;

use crate::indicators::vol::InstantVolatilityIndicator;
use crate::indicators::calculators::{VwapCalculator, DepthCalculator, PriceFitter};
use crate::indicators::trend_state::{TrendStateMachine, TrendDirection};
use crate::indicators::trend_state::TrendConfig as TrendStateConfig;
use crate::config::MonitorConfig;
use crate::stats::VolatilityStats;
use crate::models::BinanceEvent;
use crate::telemetry::{TelemetryServer, TelemetryPacket};

use futures_util::{SinkExt, StreamExt};
use tokio::time::Instant;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{info, error};
use chrono::Local;
pub async fn run_connection(
    vol_calc: &mut InstantVolatilityIndicator,
    cfg: &MonitorConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let telemetry = TelemetryServer::new(true, 9001);
    let mut stats = VolatilityStats::new(cfg.histogram.step, cfg.histogram.buckets);

    let mut last_hist_time = Instant::now();

    // 趋势计算器
    let mut vwap_calc = VwapCalculator::new(cfg.trend.vwap_window_ms, cfg.trend.vwap_series_max_len);
    let mut depth_calc = DepthCalculator::new(cfg.trend.ofi_cum_window_secs, cfg.trend.ofi_decay);
    let fitter_5s = PriceFitter::new(cfg.trend.fit_window_secs, cfg.trend.fit_min_points, cfg.trend.fit_min_r2);
    let fitter_2s = PriceFitter::new(cfg.trend.fit_window_2s, cfg.trend.fit_min_points / 2, cfg.trend.fit_min_r2);
    
    let trend_state_cfg = TrendStateConfig {
        slope_threshold: cfg.trend.slope_threshold,
        ofi_confirm_threshold: cfg.trend.ofi_confirm_threshold,
        cooldown_secs: cfg.trend.cooldown_secs,
        slope_threshold_ratio: cfg.trend.slope_threshold_ratio,
        min_price_fallback: cfg.trend.min_price_fallback,
        max_price_fallback: cfg.trend.max_price_fallback,
        entry_protection_secs: cfg.trend.entry_protection_secs,
        slope_weak_threshold: cfg.trend.slope_weak_threshold,
    };
    let mut trend_sm = TrendStateMachine::new(trend_state_cfg);
    
    let mut current_cum_ofi = 0.0;
    let mut last_fit_2s: Option<crate::indicators::calculators::FitResult> = None;
    let mut last_vol_alert_time: Option<Instant> = None;
    let mut last_agg_id: u64 = 0;  // 用于检测重复的 aggTrade 消息

    let url = "wss://fstream.binance.com/stream?streams=btcusdt@aggTrade/btcusdt@depth20@100ms";
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    info!("✅ Connected. Threshold: {:.1}%", cfg.threshold);

    while let Some(message) = read.next().await {
        if last_hist_time.elapsed().as_secs() >= cfg.histogram.interval {
            let report = stats.generate_report(cfg.histogram.interval / 60);
            notifier::send_histogram_report(cfg.slack_webhook_url.clone(), report);
            info!("📊 Histogram report sent.");
            stats = VolatilityStats::new(cfg.histogram.step, cfg.histogram.buckets);
            last_hist_time = Instant::now();
        }

        let msg = match message {
            Ok(m) => m,
            Err(e) => { error!("WS Error: {:?}", e); return Err(Box::new(e)); }
        };

        match msg {
            Message::Text(text_bytes) => {
                let text = text_bytes.as_str();
                let json_val: serde_json::Value = match serde_json::from_str(text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let event_data = json_val.get("data").unwrap_or(&json_val);
                let event: BinanceEvent = match serde_json::from_value(event_data.clone()) {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                match event {
                    BinanceEvent::Trade(trade) => {
                        // 检测重复消息
                        if trade.agg_id <= last_agg_id {
                            continue;
                        }
                        last_agg_id = trade.agg_id;

                        let p: f64 = trade.price.parse()?;
                        let q: f64 = trade.quantity.parse()?;
                        let trade_ms = trade.trade_time as u64;

                        // 波动率计算
                        vol_calc.update(p, trade_ms);
                        let vol_res = vol_calc.get_volatility();

                        // OFI 计算器添加成交
                        depth_calc.add_trade(trade_ms, p, q, trade.is_buyer_maker);

                        // VWAP 计算 + 拟合 + 状态机更新
                        if let Some(_vwap_point) = vwap_calc.add_trade(p, q, trade_ms) {
                            let current_ts_sec = trade_ms as f64 / 1000.0;
                            let fit_5s = fitter_5s.fit(vwap_calc.get_series(), trade_ms);
                            let fit_2s = fitter_2s.fit(vwap_calc.get_series(), trade_ms);
                            
                            // 保存 fit_2s 用于后续价差计算
                            last_fit_2s = fit_2s;
                            
                            let latest_price = vwap_calc.get_series().back()
                                .map(|pt| pt.price)
                                .unwrap_or(p);

                            // 状态机更新
                            trend_sm.update(
                                current_ts_sec,
                                fit_5s.as_ref(),
                                current_cum_ofi,
                                latest_price,
                            );
                        }

                        // 波动率统计
                        if vol_calc.is_ready() && !vol_res.is_stale {
                            stats.record(vol_res.annualized);
                        }

                        // 获取冲击价格
                        let impact_price = depth_calc.get_impact_price();

                        // 决定信号来源和价差调整
                        let spread_adj = cfg.volatility.spread_adjust;
                        
                        // 高波动率处理 (>= 100%)
                        if vol_res.annualized >= cfg.threshold {
                            // 发送 Slack 警报（带冷却）
                            let now = Instant::now();
                            let should_alert = last_vol_alert_time
                                .map(|t| now.duration_since(t).as_secs() >= cfg.cooldown_secs)
                                .unwrap_or(true);
                            
                            if should_alert && cfg.slack_enabled {
                                let time_str = Local::now().format("%H:%M:%S").to_string();
                                notifier::send_slack_alert(
                                    cfg.slack_webhook_url.clone(),
                                    vol_res.annualized,
                                    cfg.threshold,
                                    vol_res.raw_vol,
                                    vol_res.dt_secs,
                                    p,
                                    time_str,
                                );
                                last_vol_alert_time = Some(now);
                            }
                            
                            // 发送 Telemetry
                            telemetry.send(TelemetryPacket {
                                timestamp: trade_ms,
                                source: "V".to_string(),
                                ask_adjust: spread_adj,
                                bid_adjust: -spread_adj,
                            });
                        } else {
                            // 检查趋势
                            let direction = trend_sm.get_direction();
                            if direction != TrendDirection::Neutral {
                                // 计算预测价格与冲击价格的偏差
                                let price_diff = if let Some(ref fit) = last_fit_2s {
                                    if fit.is_valid && impact_price > 0.0 {
                                        let predicted = fitter_2s.predict(fit, 1.0);
                                        (predicted - impact_price).abs()
                                    } else {
                                        spread_adj
                                    }
                                } else {
                                    spread_adj
                                };

                                // 根据趋势方向设置价差调整
                                let (source, ask_adj, bid_adj) = match direction {
                                    TrendDirection::Long => ("U", price_diff, 0.0),
                                    TrendDirection::Short => ("D", 0.0, -price_diff),
                                    TrendDirection::Neutral => unreachable!(),
                                };

                                telemetry.send(TelemetryPacket {
                                    timestamp: trade_ms,
                                    source: source.to_string(),
                                    ask_adjust: ask_adj,
                                    bid_adjust: bid_adj,
                                });
                            }
                        }
                    }

                    BinanceEvent::Depth(depth) => {
                        // 解析订单簿
                        let bids: Vec<(f64, f64)> = depth.bids.iter()
                            .filter_map(|(p, q)| Some((p.parse().ok()?, q.parse().ok()?)))
                            .collect();
                        let asks: Vec<(f64, f64)> = depth.asks.iter()
                            .filter_map(|(p, q)| Some((p.parse().ok()?, q.parse().ok()?)))
                            .collect();

                        // 更新 OFI 状态
                        if let Some((_raw_ofi, cum_ofi, _mid_price)) = depth_calc.update_depth(
                            depth.update_id,
                            depth.trans_time,
                            &bids,
                            &asks,
                        ) {
                            current_cum_ofi = cum_ofi;
                        }
                        
                        // 计算冲击价格 (1 BTC)
                        depth_calc.calculate_impact_price(&bids, &asks, 1.0);
                    }

                }
            }
            Message::Ping(payload) => { write.send(Message::Pong(payload)).await?; }
            Message::Close(_) => { break; }
            _ => (),
        }
    }
    Ok(())
}