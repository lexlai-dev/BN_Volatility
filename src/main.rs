mod common;
mod indicators;

use crate::indicators::base::TrailingIndicator;
use crate::indicators::vol::InstantVolatilityIndicator;
use chrono::{DateTime, Local, TimeZone};
use futures_util::{StreamExt, SinkExt};
use serde::Deserialize;
use serde_json::json;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
// 定义币安推送的 AggTrade 结构
#[derive(Deserialize, Debug)]
struct AggTrade {
    #[serde(rename = "E")]
    event_time: i64,
    #[serde(rename = "p")]
    price: String,
}

#[tokio::main]
async fn main() {
    let mut vol_calc = InstantVolatilityIndicator::new(30, 15);

    // 使用基础域名
    let url = "wss://fstream.binance.com/ws";
    println!("🚀 Connecting to Binance Futures WS...");

    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // 构建订阅消息 (对应 Python 的 sub 变量)
    let subscribe_msg = json!({
        "method": "SUBSCRIBE",
        "params": [
            "btcusdt@aggTrade",
            // 如果需要 1s K线也可以加上： "btcusdt_perpetual@continuousKline_1s"
        ],
        "id": 1
    });

    // 发送订阅请求
    write.send(Message::Text(subscribe_msg.to_string().into()))
        .await
        .expect("Failed to send subscribe message");

    println!("✅ Subscription sent, waiting for trades...");

    while let Some(Ok(msg)) = read.next().await {
        if let Message::Text(text_bytes) = msg {
            let text = text_bytes.as_str();
            if text.contains("result") { continue; }

            if let Ok(trade) = serde_json::from_str::<AggTrade>(text) {
                if let Ok(p_f64) = trade.price.parse::<f64>() {
                    // 币安 event_time 是毫秒，转换为秒和纳秒
                    let datetime: DateTime<Local> = Local.timestamp_millis_opt(trade.event_time)
                        .unwrap(); // 获取本地时间

                    vol_calc.add_sample(p_f64.ln(), trade.event_time as f64 / 1000.0);

                    if vol_calc.is_sampling_buffer_full() {
                        // 使用 .format() 自定义输出格式
                        println!(
                            "[{}] Price: {:.2} | Vol: {:.4}%",
                            datetime.format("%Y-%m-%d %H:%M:%S%.3f"), // 精确到毫秒
                            p_f64,
                            vol_calc.current_value() * 100.0
                        );
                    }
                }
            }
        }
    }
}