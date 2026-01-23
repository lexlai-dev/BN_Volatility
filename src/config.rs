use serde::Deserialize;
use std::env;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct HistogramConfig {
    pub interval: u64,
    pub step: f64,
    pub buckets: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitorConfig {
    // 敏感信息（从 .env 加载，不包含在 YAML 中）
    #[serde(skip)]
    pub webhook_url: String,

    // 策略参数（来自 YAML）
    pub threshold: f64,
    pub cooldown_secs: u64,

    // 嵌套结构匹配 YAML
    pub histogram: HistogramConfig,
}

impl MonitorConfig {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        // 1. 加载 .env 文件
        dotenvy::dotenv().ok();

        // 2. 读取 config.yaml 文件
        let yaml_content = fs::read_to_string("config.yaml")
            .expect("Failed to read config.yaml. Please ensure it exists.");

        // 3. 解析 YAML
        let mut config: MonitorConfig = serde_yaml::from_str(&yaml_content)?;

        // 4. 手动从环境变量注入敏感信息
        config.webhook_url = env::var("SLACK_WEBHOOK_URL")
            .expect("SLACK_WEBHOOK_URL must be set in .env file");

        Ok(config)
    }
}