use serde_json::json;
use tracing::{info, error};

/// Sends a high-priority alert to Slack.
/// Displays the largest 1-second candle movement observed in the last 5 seconds.
pub fn send_slack_alert(
    webhook_url: String,
    vol: f64,           // 年化波动率
    threshold: f64,
    raw_vol: f64,       // 原始 RMS
    dt_secs: f64,       // 时间窗口（秒）
    signal_time: String, // 信号产生时间
    // K line data
    k_open: f64,
    k_close: f64,
    k_change: f64,
    k_volume: f64,
    k_time_str: String,
) {
    let client = reqwest::Client::new();

    let arrow = if k_change >= 0.0 { "📈" } else { "📉" };
    let sign = if k_change >= 0.0 { "+" } else { "" };
    let pct_change = (k_change / k_open) * 100.0;

    let message = format!(
        "🚨 *BTC High Volatility Alert* 🚨\n\
        > *Signal Time*: `{}`\n\
        > *Volatility*: *{:.2}%* (Threshold: {}%)\n\
        > *Raw RMS*: `{:.6}` | *Window*: `{:.3}s`\n\
        > --------------------------------\n\
        > *🕯️ Max 1s Candle (Past 5s)*:\n\
        > *Time*: `{} (1s)`\n\
        > *Open*: `${:.2}`  ➡  *Close*: `${:.2}`\n\
        > *Change*: {} `{}{:.2}` (`{}{:.3}%`)\n\
        > *Volume*: `{:.4} BTC`",
        signal_time,
        vol * 100.0, threshold,
        raw_vol, dt_secs,
        k_time_str,
        k_open, k_close,
        arrow, sign, k_change, sign, pct_change,
        k_volume
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

/// Sends a trend alert to Slack based on Order Flow Imbalance + VWAP analysis.
pub fn send_trend_alert(
    webhook_url: String,
    trend_direction: &str,  // "Bullish" or "Bearish"
    flow_imbalance: f64,    // Order Flow Imbalance (-1.0 to +1.0)
    vwap: f64,              // Volume Weighted Average Price
    vwap_bias: f64,         // VWAP deviation percentage
    current_price: f64,
    trade_count: usize,     // Number of trades in window
    time_str: String,
) {
    let client = reqwest::Client::new();

    let (arrow, direction_cn) = match trend_direction {
        "Bullish" => ("🚀", "看涨"),
        "Bearish" => ("🔻", "看跌"),
        _ => ("➡️", "中性"),
    };

    let imbalance_sign = if flow_imbalance >= 0.0 { "+" } else { "" };
    let bias_sign = if vwap_bias >= 0.0 { "+" } else { "" };

    let message = format!(
        "{} *BTC Trend Alert* {}\n\
        > *检测到{}趋势*\n\
        > --------------------------------\n\
        > *资金流向*: `{}{:.2}%` (净{})\n\
        > *VWAP*: `${:.2}`\n\
        > *当前价*: `${:.2}` (`{}{:.4}%` 偏离)\n\
        > *窗口*: 最近 `{}` 笔交易\n\
        > *时间*: `{}`",
        arrow, arrow,
        direction_cn,
        imbalance_sign, flow_imbalance * 100.0, if flow_imbalance >= 0.0 { "买入" } else { "卖出" },
        vwap,
        current_price, bias_sign, vwap_bias * 100.0,
        trade_count,
        time_str
    );

    tokio::spawn(async move {
        match client.post(webhook_url).json(&json!({"text": message})).send().await {
            Ok(_) => info!("🌊 Trend alert delivered successfully."),
            Err(e) => error!("❌ Failed to send Trend alert: {:?}", e),
        }
    });
}