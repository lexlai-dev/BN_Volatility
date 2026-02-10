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
    pub expire_threshold_ms: u64,   // 价格序列过期清除阈值（毫秒），例如 5000 = 5秒
    pub spread_adjust: f64,         // 波动率报警时调大双边价差（$），例如 10.0
}

/// 趋势监控配置（基于价格拟合 + OFI）
#[derive(Debug, Deserialize, Clone)]
pub struct TrendConfig {
    // VWAP 参数
    pub vwap_window_ms: u64,        // VWAP 聚合窗口（毫秒），例如 100
    pub vwap_series_max_len: usize, // VWAP 序列最大长度，例如 1000
    
    // 拟合参数
    pub fit_window_secs: f64,       // 拟合窗口（秒），例如 5.0
    pub fit_window_2s: f64,         // 2秒拟合窗口（用于预测），例如 2.0
    pub fit_min_points: usize,      // 最少数据点，例如 15
    pub fit_min_r2: f64,            // 最小 R²，例如 0.80
    
    // OFI 参数
    pub ofi_cum_window_secs: f64,   // OFI 累积窗口（秒），例如 1.5
    pub ofi_decay: f64,             // EMA 衰减因子，例如 0.8
    
    // 信号阈值
    pub slope_threshold: f64,       // 斜率阈值（$/s），例如 4.0
    pub ofi_confirm_threshold: f64, // OFI 确认阈值，例如 1.0
    
    // 退出参数
    pub slope_threshold_ratio: f64, // 斜率比例系数，例如 0.25
    pub min_price_fallback: f64,    // 最小价格回落（$），例如 10.0
    pub max_price_fallback: f64,    // 最大价格回落（$），例如 35.0
    pub entry_protection_secs: f64, // 入场保护期（秒），例如 1.0
    pub slope_weak_threshold: f64,  // 斜率不够明显的阈值，例如 0.5
    
    // 预测参数
    pub predict_horizon_secs: f64,  // 预测时间范围（秒），例如 1.0
    
    // 冷却
    pub cooldown_secs: f64,         // 信号冷却期（秒），例如 1.0
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitorConfig {
    // Maps directly to 'slack_webhook_url' in the YAML file.
    pub slack_webhook_url: String,
    /// Slack 报警开关，false 时不发送任何 Slack 消息
    #[serde(default = "default_slack_enabled")]
    pub slack_enabled: bool,

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

/// 默认启用 Slack 报警
fn default_slack_enabled() -> bool {
    true
}