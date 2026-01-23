mod common;
mod indicators;
mod config;
mod stats;
mod models;
mod notifier;

use crate::indicators::base::TrailingIndicator;
use crate::indicators::vol::InstantVolatilityIndicator;
use crate::config::MonitorConfig;
use crate::stats::VolatilityStats;
use crate::models::AggTrade;

use chrono::{Local, TimeZone};
use futures_util::{SinkExt, StreamExt};
use tokio::time::{sleep, Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use log::{info, warn, error, debug};

#[tokio::main]
async fn main() {
    // Initialize logging subsystem. Defaults to "info" level if RUST_LOG is not set.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 1. Load configuration ONCE at startup.
    // If this fails, we exit immediately because the application cannot function without config.
    let cfg = match MonitorConfig::load() {
        Ok(c) => c,
        Err(e) => {
            error!("❌ Critical Error: Failed to load configuration: {}", e);
            return; // Exit the application
        }
    };

    // Initialize volatility calculator (window size: 30, sampling interval: 15).
    // Defined outside the loop to preserve state across reconnections.
    let mut vol_calc = InstantVolatilityIndicator::new(30, 15);

    loop {
        info!("🚀 Starting Binance Volatility Monitor...");

        // 2. Pass the configuration by reference (&cfg) to the connection handler.
        if let Err(e) = run_connection(&mut vol_calc, &cfg).await {
            error!("⚠️ Connection lost: {:?}. Retrying in 5s...", e);
        }

        sleep(Duration::from_secs(5)).await;
    }
}

async fn run_connection(
    vol_calc: &mut InstantVolatilityIndicator,
    cfg: &MonitorConfig // Receives config as a reference
) -> Result<(), Box<dyn std::error::Error>> {

    // Initialize statistics using parameters from the loaded config.
    let mut stats = VolatilityStats::new(cfg.histogram.step, cfg.histogram.buckets);

    let mut last_hist_time = Instant::now();
    let mut last_alert_time: Option<Instant> = None;

    // Establish WebSocket connection to Binance Futures AggTrade stream.
    let url = "wss://fstream.binance.com/ws/btcusdt@aggTrade";
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    info!("✅ Connected to Binance (Threshold: {:.1}%, Hist Interval: {}s)",
             cfg.threshold, cfg.histogram.interval);

    // State variables for millisecond-level VWAP aggregation.
    let mut current_ms: Option<i64> = None;
    let mut sum_pv = 0.0;
    let mut sum_v = 0.0;

    while let Some(message) = read.next().await {

        // --- Periodic Histogram Reporting ---
        if last_hist_time.elapsed().as_secs() >= cfg.histogram.interval {
            let report = stats.generate_report(cfg.histogram.interval / 60);
            notifier::send_histogram_report(cfg.slack_webhook_url.clone(), report);

            info!("📊 Histogram report sent.");

            // Reset statistics for the next interval.
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

                    match current_ms {
                        None => {
                            current_ms = Some(trade_ms);
                            sum_pv = p * q;
                            sum_v = q;
                        }
                        Some(ms) if ms == trade_ms => {
                            // Accumulate volume and PV for the same millisecond.
                            sum_pv += p * q;
                            sum_v += q;
                        }
                        Some(ms) => {
                            // Timestamp changed: Finalize the previous millisecond logic.
                            if sum_v > 0.0 {
                                // Calculate Volume Weighted Average Price (VWAP) to reduce noise.
                                let vwap_p = sum_pv / sum_v;
                                vol_calc.add_sample(vwap_p.ln(), ms as f64 / 1000.0);

                                if vol_calc.is_sampling_buffer_full() {
                                    let current_vol = vol_calc.current_value();
                                    stats.record(current_vol);

                                    // Debug log visible only when RUST_LOG=debug.
                                    debug!("Vol: {:.4}% | Price: {:.2}", current_vol * 100.0, vwap_p);

                                    // Check threshold and trigger alert if cooldown period has passed.
                                    if current_vol >= (cfg.threshold / 100.0) {
                                        let now = Instant::now();
                                        let needs_alert = match last_alert_time {
                                            None => true,
                                            Some(last) => now.duration_since(last).as_secs() >= cfg.cooldown_secs,
                                        };

                                        if needs_alert {
                                            notifier::send_slack_alert(
                                                cfg.slack_webhook_url.clone(),
                                                vwap_p,
                                                current_vol,
                                                Local.timestamp_millis_opt(ms).unwrap().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                                                cfg.threshold
                                            );

                                            warn!("🔥 High Volatility Alert triggered! Vol: {:.2}%", current_vol * 100.0);

                                            last_alert_time = Some(now);
                                        }
                                    }
                                }
                            }
                            // Reset accumulators for the new millisecond.
                            current_ms = Some(trade_ms);
                            sum_pv = p * q;
                            sum_v = q;
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