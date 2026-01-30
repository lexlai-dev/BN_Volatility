use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

struct PriceData {
    ln_price: f64,
    timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct VolatilityResult {
    pub annualized: f64,
    pub raw_vol: f64,
    pub dt_secs: f64,
    pub duration_ms: u64,
    pub is_stale: bool,
}

pub struct InstantVolatilityIndicator {
    window_size: usize,
    prices: VecDeque<PriceData>,
    seconds_in_year: f64,
    stale_threshold_ms: u64,
    fallback_volatility: f64,
    expire_threshold_ms: u64,
}

impl InstantVolatilityIndicator {
    pub fn new(
        window_size: usize, 
        stale_threshold_ms: u64, 
        fallback_volatility: f64,
        expire_threshold_ms: u64,
    ) -> Self {
        Self {
            window_size,
            prices: VecDeque::with_capacity(window_size),
            seconds_in_year: 31536000.0,
            stale_threshold_ms,
            fallback_volatility,
            expire_threshold_ms,
        }
    }

    pub fn update(&mut self, price: f64, trade_time_ms: u64) {
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        // 清除过期数据
        while let Some(front) = self.prices.front() {
            if now_ms.saturating_sub(front.timestamp_ms) > self.expire_threshold_ms {
                self.prices.pop_front();
            } else {
                break;
            }
        }

        self.prices.push_back(PriceData { ln_price: price.ln(), timestamp_ms: trade_time_ms });

        if self.prices.len() > self.window_size {
            self.prices.pop_front();
        }
    }

    pub fn get_volatility(&self) -> VolatilityResult {
        let stale_result = VolatilityResult {
            annualized: self.fallback_volatility, raw_vol: 0.0, dt_secs: 0.0, duration_ms: 0, is_stale: true,
        };

        if self.prices.len() < 2 { return stale_result; }

        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        let latest_ts = self.prices.back().unwrap().timestamp_ms;
        if now_ms.saturating_sub(latest_ts) > self.stale_threshold_ms {
            println!("⚠️ 警告: 市场行情中断! 上次成交: {}ms 前", now_ms - latest_ts);
            return stale_result;
        }

        let ln_prices: Vec<f64> = self.prices.iter().map(|p| p.ln_price).collect();
        let count = ln_prices.len() - 1;
        let diff_sq_sum: f64 = ln_prices.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
        let raw_vol = if count > 0 { (diff_sq_sum / count as f64).sqrt() } else { 0.0 };

        let first_ts = self.prices.front().unwrap().timestamp_ms;
        let duration_ms = latest_ts.saturating_sub(first_ts);
        let dt_secs = duration_ms as f64 / 1000.0;
        let annualized = raw_vol * (self.seconds_in_year / dt_secs.max(0.01)).sqrt();

        VolatilityResult { annualized, raw_vol, dt_secs, duration_ms, is_stale: false }
    }

    pub fn is_ready(&self) -> bool { self.prices.len() >= self.window_size }
    pub fn can_calculate(&self) -> bool { self.prices.len() >= 2 }
}