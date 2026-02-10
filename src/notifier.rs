use serde_json::json;
use tracing::{info, error};

/// 发送高波动率警报到 Slack
/// 
/// # 参数
/// - `vol`: 年化波动率 (1.0 = 100%)
/// - `threshold`: 触发阈值 (%)
/// - `raw_vol`: 原始 RMS 波动率
/// - `dt_secs`: 计算窗口时长 (秒)
/// - `current_price`: 当前价格
/// - `signal_time`: 信号时间字符串
pub fn send_slack_alert(
    webhook_url: String,
    vol: f64,
    threshold: f64,
    raw_vol: f64,
    dt_secs: f64,
    current_price: f64,
    signal_time: String,
) {
    let client = reqwest::Client::new();

    let message = format!(
        "🚨 *BTC High Volatility Alert* 🚨\n\
        > *时间*: `{}`\n\
        > *波动率*: *{:.2}%* (阈值: {}%)\n\
        > *当前价*: `${:.2}`\n\
        > *原始 RMS*: `{:.6}` | *窗口*: `{:.3}s`",
        signal_time,
        vol * 100.0, threshold,
        current_price,
        raw_vol, dt_secs,
    );

    tokio::spawn(async move {
        match client.post(webhook_url).json(&json!({"text": message})).send().await {
            Ok(_) => info!("🚀 Slack alert delivered successfully."),
            Err(e) => error!("❌ Failed to send Slack alert: {:?}", e),
        }
    });
}

pub fn send_histogram_report(webhook_url: String, report: String) {
    let client = reqwest::Client::new();
    tokio::spawn(async move {
        match client.post(webhook_url).json(&json!({"text": report})).send().await {
            Ok(_) => info!("📊 Histogram delivered successfully."),
            Err(e) => error!("❌ Failed to send histogram: {:?}", e),
        }
    });
}

// Sends a trend alert to Slack based on Order Flow Imbalance + VWAP analysis.
// pub fn send_trend_alert(
//     webhook_url: String,
//     trend_direction: &str,  // "Bullish" or "Bearish"
//     flow_imbalance: f64,    // Order Flow Imbalance (-1.0 to +1.0)
//     vwap: f64,              // Volume Weighted Average Price
//     vwap_bias: f64,         // VWAP deviation percentage
//     current_price: f64,
//     trade_count: usize,     // Number of trades in window
//     time_str: String,
// ) {
//     let client = reqwest::Client::new();

//     let (arrow, direction_cn) = match trend_direction {
//         "Bullish" => ("🚀", "看涨"),
//         "Bearish" => ("🔻", "看跌"),
//         _ => ("➡️", "中性"),
//     };

//     let imbalance_sign = if flow_imbalance >= 0.0 { "+" } else { "" };
//     let bias_sign = if vwap_bias >= 0.0 { "+" } else { "" };

//     let message = format!(
//         "{} *BTC Trend Alert* {}\n\
//         > *检测到{}趋势*\n\
//         > --------------------------------\n\
//         > *资金流向*: `{}{:.2}%` (净{})\n\
//         > *VWAP*: `${:.2}`\n\
//         > *当前价*: `${:.2}` (`{}{:.4}%` 偏离)\n\
//         > *窗口*: 最近 `{}` 笔交易\n\
//         > *时间*: `{}`",
//         arrow, arrow,
//         direction_cn,
//         imbalance_sign, flow_imbalance * 100.0, if flow_imbalance >= 0.0 { "买入" } else { "卖出" },
//         vwap,
//         current_price, bias_sign, vwap_bias * 100.0,
//         trade_count,
//         time_str
//     );

//     tokio::spawn(async move {
//         match client.post(webhook_url).json(&json!({"text": message})).send().await {
//             Ok(_) => info!("🌊 Trend alert delivered successfully."),
//             Err(e) => error!("❌ Failed to send Trend alert: {:?}", e),
//         }
//     });
// }