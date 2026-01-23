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
use futures_util::{StreamExt, SinkExt};
use tokio::time::{sleep, Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let mut vol_calc = InstantVolatilityIndicator::new(30, 15);

    loop {
        println!("🚀 Connecting to BN WebSocket...");
        if let Err(e) = run_connection(&mut vol_calc).await {
            eprintln!("⚠️ Connection error: {:?}. Retrying in 5s...", e);
        }
        sleep(Duration::from_secs(5)).await;
    }
}

async fn run_connection(vol_calc: &mut InstantVolatilityIndicator) -> Result<(), Box<dyn std::error::Error>> {
    // --- 1. 加载配置 (混合模式) ---
    // 这里只在连接建立时加载一次。如果需要修改参数，重启程序即可。
    // cfg 包含了：
    // - webhook_url (来自 .env)
    // - threshold, cooldown_secs (来自 yaml)
    // - histogram { interval, step, buckets } (来自 yaml)
    let cfg = MonitorConfig::load()?;

    // --- 2. 初始化组件 ---
    // 使用 YAML 配置初始化直方图统计器
    let mut stats = VolatilityStats::new(cfg.histogram.step, cfg.histogram.buckets);

    // 计时器
    let mut last_hist_time = Instant::now();
    let mut last_alert_time: Option<Instant> = None;

    // --- 3. 建立 WebSocket 连接 ---
    let url = "wss://fstream.binance.com/ws/btcusdt@aggTrade";
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    println!("✅ Connected to Binance (Threshold: {:.1}%, Hist Interval: {}s)",
             cfg.threshold, cfg.histogram.interval);

    // --- 4. 聚合状态变量 ---
    let mut current_ms: Option<i64> = None;
    let mut sum_pv = 0.0;
    let mut sum_v = 0.0;

    // --- 5. 消息主循环 ---
    while let Some(message) = read.next().await {

        // [直方图报告逻辑]
        // 检查是否达到 YAML 中配置的 interval 时间
        if last_hist_time.elapsed().as_secs() >= cfg.histogram.interval {
            // 生成报告 (传入分钟数用于显示)
            let report = stats.generate_report(cfg.histogram.interval / 60);

            // 发送 (使用来自 .env 的 webhook_url)
            notifier::send_histogram_report(cfg.webhook_url.clone(), report);

            // 重置统计器 (使用 YAML 中的 step 和 buckets)
            stats = VolatilityStats::new(cfg.histogram.step, cfg.histogram.buckets);
            last_hist_time = Instant::now();
        }

        let msg = message?;
        match msg {
            Message::Text(text_bytes) => {
                let text = text_bytes.as_str();

                // 使用 models::AggTrade 解析
                if let Ok(trade) = serde_json::from_str::<AggTrade>(text) {
                    let p: f64 = trade.price.parse()?;
                    let q: f64 = trade.quantity.parse()?;
                    let trade_ms = trade.event_time;

                    // VWAP 毫秒级聚合
                    match current_ms {
                        None => {
                            current_ms = Some(trade_ms);
                            sum_pv = p * q;
                            sum_v = q;
                        }
                        Some(ms) if ms == trade_ms => {
                            sum_pv += p * q;
                            sum_v += q;
                        }
                        Some(ms) => {
                            // 时间戳跳变，结算上一毫秒
                            if sum_v > 0.0 {
                                let vwap_p = sum_pv / sum_v;
                                vol_calc.add_sample(vwap_p.ln(), ms as f64 / 1000.0);

                                if vol_calc.is_sampling_buffer_full() {
                                    let current_vol = vol_calc.current_value();

                                    // 记录到直方图
                                    stats.record(current_vol);

                                    // 仅在 Dev 模式下打印每毫秒数据，Release 模式下静默
                                    #[cfg(debug_assertions)]
                                    println!(
                                        "[{}] Vol: {:.4}%",
                                        Local.timestamp_millis_opt(ms).unwrap().format("%H:%M:%S%.3f"),
                                        current_vol * 100.0
                                    );

                                    // [预警触发逻辑]
                                    // 比较 YAML 中的 threshold (注意转换百分比)
                                    if current_vol >= (cfg.threshold / 100.0) {
                                        let now = Instant::now();

                                        // 检查冷却时间 (YAML 中的 cooldown_secs)
                                        let needs_alert = match last_alert_time {
                                            None => true,
                                            Some(last) => now.duration_since(last).as_secs() >= cfg.cooldown_secs,
                                        };

                                        if needs_alert {
                                            notifier::send_slack_alert(
                                                cfg.webhook_url.clone(),
                                                vwap_p,
                                                current_vol,
                                                Local.timestamp_millis_opt(ms).unwrap().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                                                cfg.threshold
                                            );
                                            last_alert_time = Some(now);
                                        }
                                    }
                                }
                            }
                            // 开启新的一毫秒
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
                println!("收到关闭帧，准备重连...");
                break;
            }
            _ => (),
        }
    }

    Ok(())
}