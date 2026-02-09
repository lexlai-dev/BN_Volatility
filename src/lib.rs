pub mod common;
pub mod indicators;
pub mod config;
pub mod stats;
pub mod models;
pub mod notifier;
pub mod telemetry;

use crate::indicators::vol::InstantVolatilityIndicator;
use crate::config::MonitorConfig;
use crate::stats::VolatilityStats;
use crate::models::BinanceEvent;
use crate::telemetry::{TelemetryServer, TelemetryPacket};

use futures_util::{SinkExt, StreamExt};
use tokio::time::Instant;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{info, warn, error};
pub async fn run_connection(
    vol_calc: &mut InstantVolatilityIndicator,
    cfg: &MonitorConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let telemetry = TelemetryServer::new(true, 9001);
    let mut stats = VolatilityStats::new(cfg.histogram.step, cfg.histogram.buckets);

    let mut last_hist_time = Instant::now();
    let mut last_alert_time: Option<Instant> = None;

    let url = "wss://fstream.binance.com/ws/btcusdt@aggTrade";
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

                if let BinanceEvent::Trade(trade) = event {
                    let p: f64 = trade.price.parse()?;
                    let q: f64 = trade.quantity.parse()?;
                    let trade_ms = trade.trade_time;

                    vol_calc.update(p, trade_ms as u64);
                    let vol_res = vol_calc.get_volatility();

                    telemetry.send(TelemetryPacket {
                        msg_type: "TRADE".to_string(),
                        timestamp: trade_ms as u64,
                        price: Some(p),
                        quantity: Some(q),
                        is_buyer_maker: Some(trade.is_buyer_maker),
                        vol: Some(vol_res.annualized),
                        imbalance: None,
                        bias: None,
                        trend_state: None,
                    });

                    if vol_calc.is_ready() && !vol_res.is_stale {
                        stats.record(vol_res.annualized);
                        if vol_res.annualized >= (cfg.threshold / 100.0) {
                            let now = Instant::now();
                            if last_alert_time.map(|t| now.duration_since(t).as_secs() >= cfg.cooldown_secs).unwrap_or(true) {
                                warn!("🔥 Alert! Vol: {:.2}%", vol_res.annualized * 100.0);
                                last_alert_time = Some(now);
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