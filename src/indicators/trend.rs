// src/indicators/trend.rs

use std::collections::VecDeque;
use crate::models::AggTrade;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrendState {
    Bullish, // 看涨
    Bearish, // 看跌
    Neutral, // 震荡/中性
}

pub struct TrendIndicator {
    // 窗口大小（例如最近 100 笔交易）
    window_size: usize,
    // 历史交易缓存
    trades: VecDeque<TradeData>,
    
    // --- 滚动累加器 (O(1) 更新的关键) ---
    // 累积主动买入量
    sum_buy_vol: f64,
    // 累积主动卖出量
    sum_sell_vol: f64,
    // 累积 (价格 * 数量)，用于算 VWAP
    sum_price_vol: f64,
    // 累积总数量，用于算 VWAP
    sum_vol: f64,

    // --- 阈值配置 ---
    cvd_threshold: f64,
    vwap_bias_threshold: f64,
}

// 内部使用的简化结构，存我们需要的数据即可
struct TradeData {
    price: f64,
    quantity: f64,
    is_buyer_maker: bool,
}

impl TrendIndicator {
    pub fn new(window_size: usize, cvd_threshold: f64, vwap_bias_threshold: f64) -> Self {
        Self {
            window_size,
            trades: VecDeque::with_capacity(window_size),
            sum_buy_vol: 0.0,
            sum_sell_vol: 0.0,
            sum_price_vol: 0.0,
            sum_vol: 0.0,
            cvd_threshold,
            vwap_bias_threshold,
        }
    }

    pub fn update(&mut self, trade: &AggTrade) -> TrendState {
        // 1. 解析数据 (把 String 转 f64，注意处理错误，这里简化为 unwrap)
        let price = trade.price.parse::<f64>().unwrap_or(0.0);
        let qty = trade.quantity.parse::<f64>().unwrap_or(0.0);
        
        // 2. 识别方向
        // Binance 规则: is_buyer_maker = true -> Taker 是卖方 -> 主动卖出
        let is_sell = trade.is_buyer_maker; 
        
        // 3. 添加新数据进累加器
        if is_sell {
            self.sum_sell_vol += qty;
        } else {
            self.sum_buy_vol += qty;
        }
        self.sum_price_vol += price * qty;
        self.sum_vol += qty;

        // 4. 维护队列 (入队)
        let new_data = TradeData { price, quantity: qty, is_buyer_maker: is_sell };
        self.trades.push_back(new_data);

        // 5. 维护窗口 (出队过期数据)
        if self.trades.len() > self.window_size {
            if let Some(old_trade) = self.trades.pop_front() {
                // 从累加器中减去旧数据
                if old_trade.is_buyer_maker {
                    self.sum_sell_vol -= old_trade.quantity;
                } else {
                    self.sum_buy_vol -= old_trade.quantity;
                }
                self.sum_price_vol -= old_trade.price * old_trade.quantity;
                self.sum_vol -= old_trade.quantity;
            }
        }

        // 6. 计算指标并判断趋势
        self.calculate_trend(price)
    }

    fn calculate_trend(&self, current_price: f64) -> TrendState {
        if self.sum_vol == 0.0 {
            return TrendState::Neutral;
        }

        // --- 指标 A: CVD (净买入量) ---
        let net_volume = self.sum_buy_vol - self.sum_sell_vol;
        
        // --- 指标 B: VWAP ---
        let vwap = self.sum_price_vol / self.sum_vol;
        let vwap_bias = (current_price - vwap) / vwap; // 偏离百分比

        // --- 融合策略：使用配置的阈值 ---
        if net_volume > self.cvd_threshold && vwap_bias > self.vwap_bias_threshold {
            TrendState::Bullish
        } else if net_volume < -self.cvd_threshold && vwap_bias < -self.vwap_bias_threshold {
            TrendState::Bearish
        } else {
            TrendState::Neutral
        }
    }

    /// 获取当前指标值，用于报警时展示
    pub fn get_metrics(&self, current_price: f64) -> (f64, f64, f64) {
        let cvd = self.sum_buy_vol - self.sum_sell_vol;
        let vwap = if self.sum_vol > 0.0 { self.sum_price_vol / self.sum_vol } else { current_price };
        let vwap_bias = if vwap > 0.0 { (current_price - vwap) / vwap } else { 0.0 };
        (cvd, vwap, vwap_bias)
    }

    /// 获取窗口内的交易笔数
    pub fn trade_count(&self) -> usize {
        self.trades.len()
    }

    /// Debug: 打印窗口内所有交易数据到 console
    pub fn debug_dump_trades(&self) {
        println!("=== Trend Window Dump ({} trades) ===", self.trades.len());
        println!("| {:>3} | {:>12} | {:>10} | {:>6} |", "#", "Price", "Qty", "Side");
        println!("|-----|--------------|------------|--------|");
        for (i, t) in self.trades.iter().enumerate() {
            let side = if t.is_buyer_maker { "SELL" } else { "BUY" };
            println!("| {:>3} | {:>12.2} | {:>10.6} | {:>6} |", i + 1, t.price, t.quantity, side);
        }
        println!("|-----|--------------|------------|--------|");
        println!("| SUM | Buy: {:.6} | Sell: {:.6} | CVD: {:.6} |",
                 self.sum_buy_vol, self.sum_sell_vol, self.sum_buy_vol - self.sum_sell_vol);
        let vwap = if self.sum_vol > 0.0 { self.sum_price_vol / self.sum_vol } else { 0.0 };
        println!("| VWAP: {:.2} | Total Vol: {:.6} |", vwap, self.sum_vol);
        println!("==========================================");
    }
}