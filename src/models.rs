use serde::Deserialize;

/// Represents a single Aggregated Trade event from the Binance WebSocket stream.
/// Fields are mapped to match the abbreviated JSON keys used by the API.
#[derive(Deserialize, Debug)]
pub struct AggTrade {
    /// Event time (Unix timestamp in milliseconds).
    #[serde(rename = "E")]
    pub event_time: i64,

    /// Trade price. Deserialized as String to prevent floating-point precision loss.
    #[serde(rename = "p")]
    pub price: String,

    /// Trade quantity. Deserialized as String.
    #[serde(rename = "q")]
    pub quantity: String,

    /// Is the buyer the market maker?
    /// true = Taker is seller (主动卖出), false = Taker is buyer (主动买入)
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}