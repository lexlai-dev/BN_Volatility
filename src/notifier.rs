use serde_json::json;
use log::{info, error};

/// Sends a high-priority alert to Slack when volatility exceeds the threshold.
pub fn send_slack_alert(webhook_url: String, price: f64, vol: f64, time_str: String, threshold: f64) {
    let client = reqwest::Client::new();

    // Construct the formatted Slack message.
    let message = format!(
        "🚨 *BTC High Volatility Alert* 🚨\n> *Time*: `{}`\n> *Price*: `${:.2}`\n> *Volatility*: *{:.2}%*\nThreshold: {}%",
        time_str, price, vol * 100.0, threshold
    );

    // Spawn an asynchronous task to send the request without blocking the main thread.
    tokio::spawn(async move {
        match client.post(webhook_url).json(&json!({"text": message})).send().await {
            Ok(_) => info!("🚀 Slack alert delivered successfully."),
            Err(e) => error!("❌ Failed to send Slack alert: {:?}", e),
        }
    });
}

/// Sends the periodic volatility histogram report to Slack.
pub fn send_histogram_report(webhook_url: String, report: String) {
    let client = reqwest::Client::new();

    tokio::spawn(async move {
        match client.post(webhook_url).json(&json!({"text": report})).send().await {
            Ok(_) => info!("📊 Histogram delivered successfully."),
            Err(e) => error!("❌ Failed to send histogram: {:?}", e),
        }
    });
}