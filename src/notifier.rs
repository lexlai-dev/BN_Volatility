use serde_json::json;

pub fn send_slack_alert(webhook_url: String, price: f64, vol: f64, time_str: String, threshold: f64) {
    let client = reqwest::Client::new();
    let message = format!(
        "🚨 *BTC 高波动预警* 🚨\n> *时间*: `{}`\n> *价格*: `${:.2}`\n> *波动率*: *{:.2}%*\n阈值: {}%",
        time_str, price, vol * 100.0, threshold
    );

    tokio::spawn(async move {
        let _ = client.post(webhook_url).json(&json!({"text": message})).send().await;
    });
}

pub fn send_histogram_report(webhook_url: String, report: String) {
    let client = reqwest::Client::new();
    tokio::spawn(async move {
        let _ = client.post(webhook_url).json(&json!({"text": report})).send().await;
    });
}