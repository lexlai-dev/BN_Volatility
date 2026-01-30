use std::collections::VecDeque;
use crate::models::AggTrade;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrendState { Bullish, Bearish, Neutral }

pub struct TrendIndicator {
    window_size: usize,
    trades: VecDeque<TradeData>,
    sum_buy_vol: f64,
    sum_sell_vol: f64,
    sum_price_vol: f64,
    sum_vol: f64,
    imbalance_threshold: f64,
    vwap_bias_threshold: f64,
    min_volume: f64,
}

struct TradeData { price: f64, quantity: f64, is_buyer_maker: bool }

impl TrendIndicator {
    pub fn new(window_size: usize, imbalance_threshold: f64, vwap_bias_threshold: f64, min_volume: f64) -> Self {
        Self {
            window_size, trades: VecDeque::with_capacity(window_size),
            sum_buy_vol: 0.0, sum_sell_vol: 0.0, sum_price_vol: 0.0, sum_vol: 0.0,
            imbalance_threshold, vwap_bias_threshold, min_volume,
        }
    }

    pub fn update(&mut self, trade: &AggTrade) -> TrendState {
        let price = trade.price.parse::<f64>().unwrap_or(0.0);
        let qty = trade.quantity.parse::<f64>().unwrap_or(0.0);
        let is_sell = trade.is_buyer_maker;
        
        if is_sell { self.sum_sell_vol += qty; } else { self.sum_buy_vol += qty; }
        self.sum_price_vol += price * qty;
        self.sum_vol += qty;

        self.trades.push_back(TradeData { price, quantity: qty, is_buyer_maker: is_sell });

        if self.trades.len() > self.window_size {
            if let Some(old) = self.trades.pop_front() {
                if old.is_buyer_maker { self.sum_sell_vol -= old.quantity; } 
                else { self.sum_buy_vol -= old.quantity; }
                self.sum_price_vol -= old.price * old.quantity;
                self.sum_vol -= old.quantity;
            }
        }
        self.calculate_trend(price)
    }

    fn calculate_trend(&self, current_price: f64) -> TrendState {
        if self.sum_vol < self.min_volume { return TrendState::Neutral; }

        let flow_imbalance = (self.sum_buy_vol - self.sum_sell_vol) / self.sum_vol;
        let vwap = self.sum_price_vol / self.sum_vol;
        let vwap_bias = (current_price - vwap) / vwap;

        if flow_imbalance > self.imbalance_threshold && vwap_bias > self.vwap_bias_threshold {
            TrendState::Bullish
        } else if flow_imbalance < -self.imbalance_threshold && vwap_bias < -self.vwap_bias_threshold {
            TrendState::Bearish
        } else {
            TrendState::Neutral
        }
    }

    pub fn get_metrics(&self, current_price: f64) -> (f64, f64, f64) {
        let flow_imbalance = if self.sum_vol > 0.0 { (self.sum_buy_vol - self.sum_sell_vol) / self.sum_vol } else { 0.0 };
        let vwap = if self.sum_vol > 0.0 { self.sum_price_vol / self.sum_vol } else { current_price };
        let vwap_bias = if vwap > 0.0 { (current_price - vwap) / vwap } else { 0.0 };
        (flow_imbalance, vwap, vwap_bias)
    }

    pub fn trade_count(&self) -> usize { self.trades.len() }

}