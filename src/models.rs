use serde::Deserialize;

// 定义统一的事件枚举，利用 serde 的 tag 自动分发
#[derive(Debug, Deserialize)]
#[serde(tag = "e")] // 根据 JSON 中的 "e" 字段判断是哪种类型
pub enum BinanceEvent {
    #[serde(rename = "aggTrade")]
    Trade(AggTrade),

    #[serde(rename = "bookTicker")]
    Book(BookTicker),
}

#[derive(Debug, Deserialize)]
pub struct AggTrade {
    #[serde(rename = "T")]
    pub trade_time: u64,
    #[serde(rename = "p")]
    pub price: String, // 保持 String，解析时转 f64
    #[serde(rename = "q")]
    pub quantity: String,
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

#[derive(Debug, Deserialize)]
pub struct BookTicker {
    #[serde(rename = "T")]
    pub trans_time: u64, // 撮合时间

    #[serde(rename = "b")]
    pub bid_price: String,
    #[serde(rename = "B")]
    pub bid_qty: String,

    #[serde(rename = "a")]
    pub ask_price: String,
    #[serde(rename = "A")]
    pub ask_qty: String,
}