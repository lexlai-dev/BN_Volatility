use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct HistogramConfig {
    pub interval: u64,
    pub step: f64,
    pub buckets: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitorConfig {
    // Maps directly to 'slack_webhook_url' in the YAML file.
    pub slack_webhook_url: String,

    pub threshold: f64,
    pub cooldown_secs: u64,

    pub histogram: HistogramConfig,
}

impl MonitorConfig {
    /// Loads configuration from the 'config.yaml' file in the current working directory.
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        // Attempt to read the file. Ensure 'config.yaml' exists in the root.
        let yaml_content = fs::read_to_string("config.yaml")
            .map_err(|_| "❌ Failed to read config.yaml. Make sure the file exists in the root directory.")?;

        let config: MonitorConfig = serde_yaml::from_str(&yaml_content)
            .map_err(|e| format!("❌ Failed to parse config.yaml: {}", e))?;

        // validation: Ensure critical fields like the webhook URL are populated.
        if config.slack_webhook_url.is_empty() {
            return Err("❌ slack_webhook_url in config.yaml is empty!".into());
        }

        Ok(config)
    }
}