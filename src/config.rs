use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct HistogramConfig {
    pub interval: u64,
    pub step: f64,
    pub buckets: usize,
}

/// 波动率计算配置
#[derive(Debug, Deserialize, Clone)]
pub struct VolatilityConfig {
    pub window_size: usize,         // 采样窗口大小（数据点数量），例如 30
    pub stale_threshold_ms: u64,    // 僵尸数据阈值（毫秒），例如 5000 = 5秒
    pub fallback_volatility: f64,   // 数据过期时返回的防御性波动率，例如 0.5 = 50%
}

/// 趋势监控配置（基于 Order Flow Imbalance + VWAP 偏离度）
#[derive(Debug, Deserialize, Clone)]
pub struct TrendConfig {
    pub enabled: bool,
    pub window_size: usize,         // 滑动窗口大小（交易笔数），例如 100
    pub imbalance_threshold: f64,   // Order Flow Imbalance 阈值，例如 0.15 表示净买入占比 > 15%
    pub vwap_bias_threshold: f64,   // VWAP 偏离度阈值，例如 0.0003 (万分之三)
    pub min_volume: f64,            // 最小成交量过滤（BTC），例如 0.01
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitorConfig {
    // Maps directly to 'slack_webhook_url' in the YAML file.
    pub slack_webhook_url: String,

    pub threshold: f64,
    pub cooldown_secs: u64,

    pub histogram: HistogramConfig,
    pub volatility: VolatilityConfig,
    pub trend: TrendConfig,
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