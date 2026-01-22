mod common;
mod indicators;

use crate::indicators::base::TrailingIndicator;
use crate::indicators::vol::InstantVolatilityIndicator;

use chrono::{Local, TimeZone};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::time::{sleep, Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[derive(Deserialize, Debug)]
struct AggTrade {
    #[serde(rename = "E")]
    event_time: i64,
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    quantity: String,
}

#[tokio::main]
async fn main() {

    dotenvy::dotenv().expect("Cannot find .env");

    // 指标放在 loop 外，重连时历史数据不会丢失 (RingBuffer 依然有效)
    let mut vol_calc = InstantVolatilityIndicator::new(30, 15);

    loop {
        println!("🚀 Connecting to BN WebSocket...");

        if let Err(e) = run_connection(&mut vol_calc).await {
            eprintln!("⚠️ 连接断开: {:?}。5秒后尝试重连...", e);
        }

        sleep(Duration::from_secs(5)).await;
    }
}

async fn run_connection(vol_calc: &mut InstantVolatilityIndicator) -> Result<(), Box<dyn std::error::Error>> {
    // --- 0. 初始配置读取 ---
    let mut webhook_url = std::env::var("SLACK_WEBHOOK_URL")?;
    let mut threshold: f64 = std::env::var("VOL_THRESHOLD")?.parse()?;
    let mut cooldown_secs: u64 = std::env::var("ALERT_COOLDOWN")?.parse()?;

    // 配置检查计时器
    let mut last_config_check = Instant::now();
    let config_check_interval = Duration::from_secs(30); // 每 30 秒查一次文件

    // 1. 建立连接
    let url = "wss://fstream.binance.com/ws/btcusdt@aggTrade";
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    println!("✅ Connected btcusdt@aggTrade (Initial Threshold: {:.2}%)", threshold);

    // 2. 状态变量
    let mut current_ms: Option<i64> = None;
    let mut sum_pv = 0.0;
    let mut sum_v = 0.0;
    let mut last_alert_time: Option<Instant> = None;

    // 3. 消息循环
    while let Some(message) = read.next().await {
        // --- [真·热更新核心逻辑] ---
        if last_config_check.elapsed() >= config_check_interval {
            // 强制重新加载 .env 文件到当前进程的环境变量中
            let _ = dotenvy::from_path(".env");

            // 检查阈值是否有变
            if let Ok(new_threshold) = std::env::var("VOL_THRESHOLD").and_then(|v| v.parse::<f64>().map_err(|_| std::env::VarError::NotPresent)) {
                if (new_threshold - threshold).abs() > f64::EPSILON {
                    println!("🔄 Config Reloaded: Threshold {}% -> {}%", threshold, new_threshold);
                    threshold = new_threshold;
                }
            }

            // 更新冷却时间和 URL
            if let Ok(new_cooldown) = std::env::var("ALERT_COOLDOWN").and_then(|v| v.parse::<u64>().map_err(|_| std::env::VarError::NotPresent)) {
                cooldown_secs = new_cooldown;
            }
            if let Ok(new_url) = std::env::var("SLACK_WEBHOOK_URL") {
                webhook_url = new_url;
            }

            last_config_check = Instant::now();
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
                            sum_pv += p * q;
                            sum_v += q;
                        }
                        Some(ms) => {
                            if sum_v > 0.0 {
                                let vwap_p = sum_pv / sum_v;
                                vol_calc.add_sample(vwap_p.ln(), ms as f64 / 1000.0);

                                if vol_calc.is_sampling_buffer_full() {
                                    let current_vol = vol_calc.current_value();
                                    let dt = Local.timestamp_millis_opt(ms).unwrap();

                                    println!(
                                        "[{}] Price: {:.2} | Vol: {:.4}%",
                                        dt.format("%Y-%m-%d %H:%M:%S%.3f"),
                                        vwap_p,
                                        current_vol * 100.0
                                    );

                                    // 使用实时更新后的变量进行判断
                                    if current_vol >= (threshold / 100.0) {
                                        let now = Instant::now();
                                        let needs_alert = match last_alert_time {
                                            None => true,
                                            Some(last) => now.duration_since(last).as_secs() >= cooldown_secs,
                                        };

                                        if needs_alert {
                                            send_slack_alert(
                                                webhook_url.clone(),
                                                vwap_p,
                                                current_vol,
                                                dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string(), // 加上毫秒显示
                                                threshold
                                            );
                                            last_alert_time = Some(now);
                                        }
                                    }
                                }
                            }
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

fn send_slack_alert(webhook_url: String, price: f64, vol: f64, time_str: String, threshold: f64) {
    let client = reqwest::Client::new();

    // 构建美化后的 Slack 消息
    let message = format!(
        "🚨 *BTC 高波动预警* 🚨\n\
        > *发生时间*: `{}`\n\
        > *成交价格*: `${:.2}`\n\
        > *年化波动率*: *{:.2}%*\n\
        请检查策略逻辑或仓位！目前threshold={threshold}%",
        time_str, price, vol * 100.0
    );

    let payload = json!({ "text": message });

    tokio::spawn(async move {
        match client.post(webhook_url).json(&payload).send().await {
            Ok(_) => println!("🚀 Slack 预警已送达"),
            Err(e) => eprintln!("❌ Slack 发送失败: {:?}", e),
        }
    });
}