use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct AggTrade {
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "q")]
    pub quantity: String,
}