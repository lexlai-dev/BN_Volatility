use std::collections::VecDeque;

/// Represents a standard OHLCV Candlestick.
#[derive(Debug, Clone)]
pub struct Kline {
    pub open_time: i64, // Unix timestamp (seconds)
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl Kline {
    pub fn new(time_sec: i64, price: f64, volume: f64) -> Self {
        Self {
            open_time: time_sec,
            open: price,
            high: price,
            low: price,
            close: price,
            volume,
        }
    }

    pub fn update(&mut self, price: f64, volume: f64) {
        self.close = price;
        if price > self.high { self.high = price; }
        if price < self.low { self.low = price; }
        self.volume += volume;
    }

    pub fn change(&self) -> f64 {
        self.close - self.open
    }
}

/// Manages the synthesis of 1-second Klines.
pub struct KlineManager {
    pub current: Option<Kline>,
    pub history: VecDeque<Kline>,
    history_limit: usize,
}

impl KlineManager {
    pub fn new(history_limit: usize) -> Self {
        Self {
            current: None,
            history: VecDeque::with_capacity(history_limit),
            history_limit,
        }
    }

    /// 更新价格。如果这笔交易导致上一秒的 K 线完结，返回 Some(CompletedKline)。
    pub fn update(&mut self, price: f64, volume: f64, trade_time_sec: i64) -> Option<Kline> {
        let mut completed_kline = None;

        match self.current {
            Some(ref mut k) if k.open_time == trade_time_sec => {
                // 还是同一秒，只更新数据
                k.update(price, volume);
            }
            Some(ref old_k) => {
                // 新的一秒开始了
                // 1. 保存旧的 K 线
                let finished = old_k.clone();
                completed_kline = Some(finished.clone());

                // 2. 存入历史
                if self.history.len() >= self.history_limit {
                    self.history.pop_front();
                }
                self.history.push_back(finished);

                // 3. 开启新 K 线
                self.current = Some(Kline::new(trade_time_sec, price, volume));
            }
            None => {
                // 第一次初始化
                self.current = Some(Kline::new(trade_time_sec, price, volume));
            }
        }

        completed_kline
    }

    /// 辅助函数：找出过去 N 秒内实体变化最大的 K 线 (用于波动率报警展示)
    pub fn find_max_impact_candle(&self, lookback_secs: i64, current_sec: i64) -> Option<&Kline> {
        self.history.iter()
            .chain(self.current.iter())
            .filter(|k| k.open_time >= current_sec - lookback_secs)
            .max_by(|a, b| a.change().abs().partial_cmp(&b.change().abs()).unwrap())
    }
}