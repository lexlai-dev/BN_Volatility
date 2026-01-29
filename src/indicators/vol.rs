use std::cmp::max;
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// 存储带时间戳的价格数据
struct PriceData {
    ln_price: f64,      // log(price)，用于计算 log-return
    timestamp_ms: u64,  // 毫秒级时间戳（来自 AggTrade event_time）
}

/// 波动率计算结果
#[derive(Debug, Clone, Copy)]
pub struct VolatilityResult {
    pub annualized: f64,   // 年化波动率
    pub raw_vol: f64,      // 原始 RMS (log-return 的均方根)
    pub dt_secs: f64,      // 时间窗口（秒）
    pub duration_ms: u64,  // 时间窗口（毫秒，更精确）
    pub is_stale: bool,    // 是否为僵尸数据（数据过期）
}

/// Calculates Instant Volatility (Realized Volatility) over a sliding window.
/// Includes liveness check to detect stale/zombie data.
pub struct InstantVolatilityIndicator {
    window_size: usize,
    prices: VecDeque<PriceData>,
    
    // 年化常数: 365 * 24 * 60 * 60
    seconds_in_year: f64,
    
    // 僵尸数据检查阈值（毫秒）
    stale_threshold_ms: u64,
    // 数据过期时返回的防御性波动率
    fallback_volatility: f64,
}

impl InstantVolatilityIndicator {
    pub fn new(window_size: usize, stale_threshold_ms: u64, fallback_volatility: f64) -> Self {
        Self {
            window_size,
            prices: VecDeque::with_capacity(window_size),
            seconds_in_year: 31536000.0,
            stale_threshold_ms,
            fallback_volatility,
        }
    }

    /// 更新价格数据
    /// - price: 原始价格（内部会取 ln）
    /// - event_time_ms: AggTrade 的 event_time（毫秒）
    pub fn update(&mut self, price: f64, event_time_ms: u64) {
        let ln_price = price.ln();
        self.prices.push_back(PriceData { 
            ln_price, 
            timestamp_ms: event_time_ms 
        });
        if self.prices.len() > self.window_size {
            self.prices.pop_front();
        }
    }

    /// 获取当前波动率
    /// 返回 VolatilityResult 包含年化值、原始值、时间窗口和是否过期
    pub fn get_volatility(&self) -> VolatilityResult {
        // 数据不足时，返回高风险值
        if self.prices.len() < 2 {
            return VolatilityResult {
                annualized: self.fallback_volatility,
                raw_vol: 0.0,
                dt_secs: 0.0,
                duration_ms: 0,
                is_stale: true,
            };
        }

        // --- 僵尸数据检查 (Liveness Check) ---
        if let Some(latest) = self.prices.back() {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            
            if now > latest.timestamp_ms && (now - latest.timestamp_ms) > self.stale_threshold_ms {
                println!("⚠️ 警告: 市场行情中断! 上次成交: {}ms 前", now - latest.timestamp_ms);
                return VolatilityResult {
                    annualized: self.fallback_volatility,
                    raw_vol: 0.0,
                    dt_secs: 0.0,
                    duration_ms: 0,
                    is_stale: true,
                };
            }
        }

        // --- RMS 计算 ---
        // 算法：Sqrt( Sum( (ln(Pt) - ln(Pt-1))^2 ) / Count )
        // 注意：分母是差分数量 (len-1)，不是价格数量
        let ln_prices: Vec<f64> = self.prices.iter().map(|p| p.ln_price).collect();
        let count = ln_prices.len() - 1; // 差分数量
        
        let diff_sq_sum: f64 = ln_prices.windows(2)
            .map(|w| (w[1] - w[0]).powi(2))
            .sum();
        
        let raw_vol = if count > 0 {
            (diff_sq_sum / count as f64).sqrt()
        } else {
            0.0
        };

        // --- 时间跨度计算 ---
        let first_ts = self.prices.front().unwrap().timestamp_ms;
        let last_ts = self.prices.back().unwrap().timestamp_ms;
        let duration_ms = last_ts.saturating_sub(first_ts); // 防止时间戳回退
        let dt_secs = duration_ms as f64 / 1000.0;

        // // --- 年化计算 ---
        // let annualized = if dt_secs < 0.001 {
        //     0.0
        // } else {
        //     raw_vol * (self.seconds_in_year / dt_secs).sqrt()
        // };

        let annualized = raw_vol * (self.seconds_in_year / dt_secs.max(0.001)).sqrt();
        VolatilityResult {
            annualized,
            raw_vol,
            dt_secs,
            duration_ms,
            is_stale: false,
        }
    }

    /// 检查是否已预热完成（窗口填满）
    pub fn is_ready(&self) -> bool {
        self.prices.len() >= self.window_size
    }

    /// 检查是否有足够数据进行计算（至少 2 个点）
    pub fn can_calculate(&self) -> bool {
        self.prices.len() >= 2
    }
}