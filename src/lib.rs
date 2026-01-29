// src/lib.rs

pub mod common;
pub mod indicators;
pub mod config;
pub mod stats;
pub mod models;
pub mod notifier;

use crate::indicators::vol::InstantVolatilityIndicator;
use crate::indicators::trend::{TrendIndicator, TrendState};
use crate::config::MonitorConfig;
use crate::stats::VolatilityStats;
use crate::models::AggTrade;

use chrono::{TimeZone, FixedOffset, Local};
use futures_util::{SinkExt, StreamExt};
use tokio::time::{Instant};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{info, warn};
use std::collections::VecDeque;

/// Represents a 1-second Candlestick (Kline) used for visualization in alerts.
#[derive(Debug, Clone)]
struct Kline {
    open_time: i64, // Unix timestamp in seconds
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

impl Kline {
    /// Constructs a new Kline candle.
    fn new(time_sec: i64, price: f64, volume: f64) -> Self {
        Self {
            open_time: time_sec,
            open: price,
            high: price,
            low: price,
            close: price,
            volume,
        }
    }

    /// Updates the current candle with a new trade aggregation.
    fn update(&mut self, price: f64, volume: f64) {
        self.close = price;
        if price > self.high { self.high = price; }
        if price < self.low { self.low = price; }
        self.volume += volume;
    }

    /// Calculates the price change of the candle body (Close - Open).
    fn change(&self) -> f64 {
        self.close - self.open
    }
}

/// Main logic loop for the volatility monitor.
/// Establishes the WebSocket connection, processes trades, and manages alerts.
pub async fn run_connection(
    vol_calc: &mut InstantVolatilityIndicator,
    cfg: &MonitorConfig
) -> Result<(), Box<dyn std::error::Error>> {

    let mut stats = VolatilityStats::new(cfg.histogram.step, cfg.histogram.buckets);

    // 初始化趋势指标器
    let mut trend_calc = TrendIndicator::new(
        cfg.trend.window_size,
        cfg.trend.imbalance_threshold,
        cfg.trend.vwap_bias_threshold,
        cfg.trend.min_volume
    );

    let mut last_hist_time = Instant::now();
    let mut last_alert_time: Option<Instant> = None;
    let mut last_trend_alert_time: Option<Instant> = None;

    let url = "wss://fstream.binance.com/ws/btcusdt@aggTrade";
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    info!("✅ Connected to Binance (Threshold: {:.1}%, Hist Interval: {}s)",
             cfg.threshold, cfg.histogram.interval);

    // State variables for 1-second Kline synthesis.
    let mut current_kline: Option<Kline> = None;
    // Buffer to store the last 10 completed 1s candles, ensuring we cover the 5s lookback window.
    let mut kline_history: VecDeque<Kline> = VecDeque::with_capacity(10);

    let china_timezone = FixedOffset::east_opt(8 * 3600).unwrap();
    while let Some(message) = read.next().await {
        // --- Periodic Histogram Reporting ---
        if last_hist_time.elapsed().as_secs() >= cfg.histogram.interval {
            let report = stats.generate_report(cfg.histogram.interval / 60);
            notifier::send_histogram_report(cfg.slack_webhook_url.clone(), report);
            info!("📊 Histogram report sent.");
            stats = VolatilityStats::new(cfg.histogram.step, cfg.histogram.buckets);
            last_hist_time = Instant::now();
        }

        let msg = message?;
        match msg {
            Message::Text(text_bytes) => {
                let text = text_bytes.as_str();

                if let Ok(trade) = serde_json::from_str::<AggTrade>(text) {
                    let p: f64 = trade.price.parse()?;
                    let q: f64 = trade.quantity.parse()?;
                    let trade_ms = trade.event_time;
                    let trade_sec = trade_ms / 1000;


                    // --- Trend Detection (CVD + VWAP) ---
                    let mut trend_state = TrendState::Neutral;
                    let mut flow_imbalance = 0.0;
                    let mut vwap_bias = 0.0;
                    
                    if cfg.trend.enabled {
                        trend_state = trend_calc.update(&trade);
                        let metrics = trend_calc.get_metrics(p);
                        flow_imbalance = metrics.0;
                        // metrics.1 是 vwap，当前 debug 打印中未使用
                        vwap_bias = metrics.2;

                        // 只在检测到非中性趋势时报警
                        if trend_state != TrendState::Neutral {
                            let now = Instant::now();
                            let needs_alert = match last_trend_alert_time {
                                None => true,
                                Some(last) => now.duration_since(last).as_secs() >= cfg.cooldown_secs,
                            };

                            if needs_alert {
                                let (flow_imbalance, vwap, vwap_bias) = trend_calc.get_metrics(p);
                                let direction = match trend_state {
                                    TrendState::Bullish => "Bullish",
                                    TrendState::Bearish => "Bearish",
                                    _ => "Neutral",
                                };

                                let time_str = china_timezone
                                    .timestamp_opt(trade_sec, 0).unwrap()
                                    .format("%H:%M:%S").to_string();

                                notifier::send_trend_alert(
                                    cfg.slack_webhook_url.clone(),
                                    direction,
                                    flow_imbalance,
                                    vwap,
                                    vwap_bias,
                                    p,
                                    trend_calc.trade_count(),
                                    time_str
                                );

                                let direction_cn = if trend_state == TrendState::Bullish { "看涨" } else { "看跌" };
                                warn!("🌊 Trend Alert! {} | Imbalance: {:.2}% | VWAP Bias: {:.4}%",
                                      direction_cn, flow_imbalance * 100.0, vwap_bias * 100.0);
                                
                                // Debug: 打印窗口内交易数据到 console
                                #[cfg(debug_assertions)]
                                trend_calc.debug_dump_trades();
                                
                                last_trend_alert_time = Some(now);
                            }
                        }
                    }

                    // --- 1s Kline Synthesis Logic ---
                    match current_kline {
                        Some(ref mut k) if k.open_time == trade_sec => {
                            // Same second: update current candle statistics.
                            k.update(p, q);
                        }
                        Some(old_k) => {
                            // New second detected:
                            // 1. Archive the completed candle.
                            if kline_history.len() >= 10 {
                                kline_history.pop_front();
                            }
                            kline_history.push_back(old_k);
                            // 2. Initialize a new candle.
                            current_kline = Some(Kline::new(trade_sec, p, q));
                        }
                        None => {
                            // Initialize the very first candle.
                            current_kline = Some(Kline::new(trade_sec, p, q));
                        }
                    }

                    // --- Volatility Calculation ---
                    // 每笔交易都更新波动率计算器
                    vol_calc.update(p, trade_ms as u64);
                    
                    // 获取波动率结果
                    let vol_result = vol_calc.get_volatility();
                    
                    if vol_calc.is_ready() && !vol_result.is_stale {
                        stats.record(vol_result.annualized);

                        // 计算完成时刻（本地时间）
                        let signal_time_str = Local::now().format("%H:%M:%S%.3f").to_string();

                        // Debug: 合并打印趋势+波动率+时间
                        #[cfg(debug_assertions)]
                        println!("[{}] 📊 Vol: {:.2}% (raw:{:.6}, dt:{:.3}s) | Trend: {:?} Imb:{:+.1}% Bias:{:+.4}% | P:{:.2}",
                                 signal_time_str,
                                 vol_result.annualized * 100.0, vol_result.raw_vol, vol_result.dt_secs,
                                 trend_state, flow_imbalance * 100.0, vwap_bias * 100.0, p);

                        // --- Alert Logic ---
                        if vol_result.annualized >= (cfg.threshold / 100.0) {
                            let now = Instant::now();
                            let needs_alert = match last_alert_time {
                                None => true,
                                Some(last) => now.duration_since(last).as_secs() >= cfg.cooldown_secs,
                            };

                            if needs_alert {
                                // Identify the 1s candle with the largest body change in the last 5 seconds.
                                let target_sec = trade_sec;

                                // Collect candidates: history + current incomplete candle.
                                let candidates = kline_history.iter()
                                    .chain(current_kline.iter())
                                    .filter(|k| k.open_time >= target_sec - 5);

                                // Find the candle with the maximum absolute price change.
                                if let Some(max_kline) = candidates.max_by(|a, b| a.change().abs().partial_cmp(&b.change().abs()).unwrap()) {

                                    let kline_time_str = china_timezone.timestamp_opt(max_kline.open_time, 0)
                                        .unwrap()
                                        .format("%H:%M:%S")
                                        .to_string();

                                    notifier::send_slack_alert(
                                        cfg.slack_webhook_url.clone(),
                                        vol_result.annualized,
                                        cfg.threshold,
                                        vol_result.raw_vol,
                                        vol_result.dt_secs,
                                        signal_time_str.clone(),  // 信号产生时间
                                        max_kline.open,
                                        max_kline.close,
                                        max_kline.change(),
                                        max_kline.volume,
                                        kline_time_str
                                    );

                                    warn!("🔥 Alert! Vol: {:.2}% (raw: {:.6}, dt: {:.3}s), Max 1s Candle: {:.2} ({:.2})",
                                        vol_result.annualized * 100.0, vol_result.raw_vol, vol_result.dt_secs,
                                        max_kline.change(), max_kline.volume);
                                }

                                last_alert_time = Some(now);
                            }
                        }
                    }
                }
            }
            Message::Ping(payload) => {
                write.send(Message::Pong(payload)).await?;
            }
            Message::Close(_) => {
                warn!("Received Close Frame from server.");
                break;
            }
            _ => (),
        }
    }
    Ok(())
}