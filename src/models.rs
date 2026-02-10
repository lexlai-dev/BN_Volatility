//! 币安 WebSocket 数据模型
//! 
//! 定义从币安 WebSocket 接收的事件类型和数据结构。
//! 使用 serde 进行 JSON 反序列化，字段名通过 rename 映射到币安 API 的字段。

use serde::Deserialize;

/// 币安 WebSocket 事件枚举
/// 
/// 使用 `#[serde(tag = "e")]` 根据 JSON 中的 "e" 字段自动选择变体：
/// - "aggTrade" -> Trade(AggTrade)
/// - "depthUpdate" -> Depth(DepthUpdate)
#[derive(Debug, Deserialize)]
#[serde(tag = "e")]
pub enum BinanceEvent {
    #[serde(rename = "aggTrade")]
    Trade(AggTrade),

    #[serde(rename = "depthUpdate")]
    Depth(DepthUpdate),
}

/// 聚合成交数据 (aggTrade)
/// 
/// 币安将同一价格、同一方向的连续成交聚合为一条记录。
/// 
/// # 字段
/// - `agg_id`: 聚合成交 ID，用于检测重复消息
/// - `trade_time`: 成交时间戳 (毫秒)
/// - `price`: 成交价格 (字符串，需解析为 f64)
/// - `quantity`: 成交数量
/// - `is_buyer_maker`: true = 卖单主动成交 (价格下跌方向)
#[derive(Debug, Deserialize)]
pub struct AggTrade {
    #[serde(rename = "a")]
    pub agg_id: u64,
    #[serde(rename = "T")]
    pub trade_time: u64,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "q")]
    pub quantity: String,
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

/// 深度更新数据 (depth20@100ms)
/// 
/// 每 100ms 推送一次订单簿快照，包含买卖各 20 档。
/// 
/// # 字段
/// - `trans_time`: 事务时间戳 (毫秒)
/// - `update_id`: 更新序号，用于检测数据连续性
/// - `bids`: 买单列表 [(价格, 数量), ...]，按价格降序
/// - `asks`: 卖单列表 [(价格, 数量), ...]，按价格升序
#[derive(Debug, Deserialize)]
pub struct DepthUpdate {
    #[serde(rename = "T")]
    pub trans_time: u64,
    #[serde(rename = "u")]
    pub update_id: u64,
    #[serde(rename = "b")]
    pub bids: Vec<(String, String)>,
    #[serde(rename = "a")]
    pub asks: Vec<(String, String)>,
}