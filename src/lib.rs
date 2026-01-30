pub mod common;
pub mod indicators;
pub mod config;
pub mod stats;
pub mod models;
pub mod notifier;
pub mod telemetry;

use crate::indicators::vol::InstantVolatilityIndicator;
use crate::indicators::trend::{TrendIndicator, TrendState};
use crate::config::MonitorConfig;
use crate::stats::VolatilityStats;
use crate::models::{AggTrade, BinanceEvent}; // 确保 models.rs 定义了这些
use crate::telemetry::{TelemetryServer, TelemetryPacket};

use chrono::{FixedOffset, Local, TimeZone};
use futures_util::{SinkExt, StreamExt};
use tokio::time::Instant;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{info, warn, error};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
struct Kline {
    open_time: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

impl Kline {
    fn new(time_sec: i64, price: f64, volume: f64) -> Self {
        Self { open_time: time_sec, open: price, high: price, low: price, close: price, volume }
    }

    fn update(&mut self, price: f64, volume: f64) {
        self.close = price;
        self.high = self.high.max(price);
        self.low = self.low.min(price);
        self.volume += volume;
    }

    fn change(&self) -> f64 { self.close - self.open }
}

pub async fn run_connection(
    vol_calc_trade: &mut InstantVolatilityIndicator,
    vol_calc_book: &mut InstantVolatilityIndicator,
    cfg: &MonitorConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. 启动遥测服务器
    let telemetry = TelemetryServer::new(true, 9001);

    // 2. 初始化统计和指标
    let mut stats = VolatilityStats::new(cfg.histogram.step, cfg.histogram.buckets);

    let mut trend_calc = TrendIndicator::new(
        cfg.trend.window_size,
        cfg.trend.imbalance_threshold,
        cfg.trend.vwap_bias_threshold,
        cfg.trend.min_volume,
    );

    // 内部初始化 Book 波动率计算器
    // 3. 状态与计时器
    let mut last_hist_time = Instant::now();
    let mut last_alert_time: Option<Instant> = None;
    let mut last_trend_alert_time: Option<Instant> = None;

    // 限流计时器 (仅用于 BookTicker，防止前端过载)
    let mut last_book_send_time = Instant::now();

    // 4. 连接币安 WebSocket (组合流)
    let url = "wss://fstream.binance.com/stream?streams=btcusdt@aggTrade/btcusdt@bookTicker";
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    info!("✅ Connected to Binance (Decoupled Stream). Threshold: {:.1}%", cfg.threshold);

    // K线状态
    let mut current_kline: Option<Kline> = None;
    let mut kline_history: VecDeque<Kline> = VecDeque::with_capacity(10);
    let china_timezone = FixedOffset::east_opt(8 * 3600).unwrap();

    while let Some(message) = read.next().await {
        // --- 周期性任务: 发送直方图报告 ---
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

                // 解析外层 JSON: {"stream": "...", "data": {...}}
                let json_val: serde_json::Value = match serde_json::from_str(text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let event_data = json_val.get("data").unwrap_or(&json_val);

                // 解析事件类型
                let event: BinanceEvent = match serde_json::from_value(event_data.clone()) {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                match event {
                    // ==========================================
                    // 分支 A: 处理成交 (AggTrade)
                    // ==========================================
                    BinanceEvent::Trade(trade) => {
                        let p: f64 = trade.price.parse()?;
                        let q: f64 = trade.quantity.parse()?;
                        let trade_ms = trade.trade_time;
                        let trade_sec = trade_ms / 1000;

                        // 1. 更新趋势指标
                        let mut trend_state = TrendState::Neutral;
                        let (mut flow_imb, mut vwap_bias) = (0.0, 0.0);

                        if cfg.trend.enabled {
                            trend_state = trend_calc.update(&trade);
                            let metrics = trend_calc.get_metrics(p);
                            flow_imb = metrics.0;
                            vwap_bias = metrics.2;

                            // 趋势报警逻辑
                            if trend_state != TrendState::Neutral {
                                let now = Instant::now();
                                let needs_alert = match last_trend_alert_time {
                                    None => true,
                                    Some(last) => now.duration_since(last).as_secs() >= cfg.cooldown_secs
                                };
                                if needs_alert {
                                    let direction = if trend_state == TrendState::Bullish { "Bullish" } else { "Bearish" };
                                    let time_str = china_timezone.timestamp_opt(trade_sec as i64, 0).unwrap().format("%H:%M:%S").to_string();
                                    if cfg.slack_enabled {
                                        notifier::send_trend_alert(cfg.slack_webhook_url.clone(), direction, flow_imb, metrics.1, vwap_bias, p, trend_calc.trade_count(), time_str);
                                    }
                                    warn!("🌊 Trend Alert! {} | Imbalance: {:.2}%", direction, flow_imb * 100.0);
                                    last_trend_alert_time = Some(now);
                                }
                            }
                        }

                        // 2. 更新 K 线
                        match current_kline {
                            Some(ref mut k) if k.open_time == (trade_sec as i64) => k.update(p, q),
                            Some(old_k) => {
                                if kline_history.len() >= 10 { kline_history.pop_front(); }
                                kline_history.push_back(old_k);
                                current_kline = Some(Kline::new(trade_sec as i64, p, q));
                            }
                            None => current_kline = Some(Kline::new(trade_sec as i64, p, q)),
                        }

                        // 3. 更新并获取 Trade 波动率
                        vol_calc_trade.update(p, trade_ms as u64);
                        let vol_res = vol_calc_trade.get_volatility();

                        // 4. 【发送 TRADE 消息】
                        // 此消息只包含 Trade 相关数据，Book 数据置为 None
                        telemetry.send(TelemetryPacket {
                            msg_type: "TRADE".to_string(),
                            timestamp: trade_ms as u64,

                            price: Some(p),
                            quantity: Some(q),
                            is_buyer_maker: Some(trade.is_buyer_maker),

                            vol_trade: Some(vol_res.annualized), // 有值
                            vol_book: None,                      // 空

                            trend_imbalance: Some(flow_imb),
                            vwap_bias: Some(vwap_bias),
                            trend_state: Some(match trend_state {
                                TrendState::Bullish => 1, TrendState::Bearish => -1, _ => 0,
                            }),
                        });

                        // 5. 波动率报警 (仅基于成交)
                        if vol_calc_trade.is_ready() && !vol_res.is_stale {
                            stats.record(vol_res.annualized);
                            if vol_res.annualized >= (cfg.threshold / 100.0) {
                                let now = Instant::now();
                                if last_alert_time.map(|t| now.duration_since(t).as_secs() >= cfg.cooldown_secs).unwrap_or(true) {
                                    // 简化的报警日志，实际可复用之前的 notifier 调用
                                    warn!("🔥 Alert! Trade Vol: {:.2}%", vol_res.annualized * 100.0);
                                    last_alert_time = Some(now);
                                }
                            }
                        }
                    },

                    // ==========================================
                    // 分支 B: 处理盘口 (BookTicker)
                    // ==========================================
                    BinanceEvent::Book(book) => {
                        if let (Ok(bid_p), Ok(bid_q), Ok(ask_p), Ok(ask_q)) = (
                            book.bid_price.parse::<f64>(), book.bid_qty.parse::<f64>(),
                            book.ask_price.parse::<f64>(), book.ask_qty.parse::<f64>(),
                        ) {
                            let weight_sum = ask_q + bid_q;
                            if weight_sum > 0.0 {
                                // 1. 计算加权中间价
                                let wmp = (ask_p * bid_q + bid_p * ask_q) / weight_sum;

                                // 2. 更新 Book 波动率
                                vol_calc_book.update(wmp, book.trans_time);

                                // 3. 【发送 BOOK 消息】 (带限流 100ms)
                                if last_book_send_time.elapsed().as_millis() > 1 {
                                    let vol_res = vol_calc_book.get_volatility();

                                    // 此消息只包含 Book 波动率，其他 Trade 相关字段置为 None
                                    telemetry.send(TelemetryPacket {
                                        msg_type: "BOOK".to_string(),
                                        timestamp: book.trans_time,

                                        price: None,           // 空
                                        quantity: None,        // 空
                                        is_buyer_maker: None,  // 空

                                        vol_trade: None,       // 空
                                        vol_book: Some(vol_res.annualized), // 有值

                                        trend_imbalance: None, // 空
                                        vwap_bias: None,       // 空
                                        trend_state: None,     // 空
                                    });

                                    last_book_send_time = Instant::now();
                                }
                            }
                        }
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