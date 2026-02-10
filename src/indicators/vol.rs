//! 瞬时波动率计算器
//!
//! 基于对数收益率的 RMS (均方根) 计算瞬时波动率，并年化。
//! 
//! # 算法原理
//! 1. 对每笔成交价格取自然对数: ln(price)
//! 2. 计算相邻对数价格的差值 (对数收益率): r_i = ln(p_i) - ln(p_{i-1})
//! 3. 计算 RMS: raw_vol = sqrt(Σr_i² / n)
//! 4. 年化: annualized = raw_vol * sqrt(seconds_in_year / dt)

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// 价格数据点，存储对数价格和时间戳
struct PriceData {
    ln_price: f64,      // 价格的自然对数
    timestamp_ms: u64,  // 成交时间戳 (毫秒)
}

/// 波动率计算结果
#[derive(Debug, Clone, Copy)]
pub struct VolatilityResult {
    pub annualized: f64,   // 年化波动率 (1.0 = 100%)
    pub raw_vol: f64,      // 原始 RMS 波动率
    pub dt_secs: f64,      // 数据窗口时长 (秒)
    pub duration_ms: u64,  // 数据窗口时长 (毫秒)
    pub is_stale: bool,    // 数据是否过期 (市场中断)
}

/// 瞬时波动率指标计算器
/// 
/// # 使用方式
/// ```ignore
/// let mut vol = InstantVolatilityIndicator::new(100, 5000, 0.5, 10000);
/// vol.update(price, timestamp_ms);
/// let result = vol.get_volatility();
/// ```
pub struct InstantVolatilityIndicator {
    window_size: usize,              // 滑动窗口大小 (数据点数量)
    prices: VecDeque<PriceData>,     // 价格缓冲区 (VecDeque 支持高效的头尾操作)
    seconds_in_year: f64,            // 一年的秒数，用于年化
    stale_threshold_ms: u64,         // 数据过期阈值 (毫秒)，超过则认为市场中断
    fallback_volatility: f64,        // 数据过期时返回的防御性波动率
    expire_threshold_ms: u64,        // 清除过期数据的阈值 (毫秒)
}

impl InstantVolatilityIndicator {
    /// 创建新的波动率计算器
    /// 
    /// # 参数
    /// - `window_size`: 滑动窗口大小
    /// - `stale_threshold_ms`: 数据过期阈值
    /// - `fallback_volatility`: 过期时的防御性波动率
    /// - `expire_threshold_ms`: 清除过期数据的阈值
    pub fn new(
        window_size: usize, 
        stale_threshold_ms: u64, 
        fallback_volatility: f64,
        expire_threshold_ms: u64,
    ) -> Self {
        Self {
            window_size,
            prices: VecDeque::with_capacity(window_size),
            seconds_in_year: 31536000.0,  // 365 * 24 * 3600
            stale_threshold_ms,
            fallback_volatility,
            expire_threshold_ms,
        }
    }

    /// 添加新的价格数据点
    /// 
    /// # 参数
    /// - `price`: 成交价格
    /// - `trade_time_ms`: 成交时间戳 (毫秒)
    pub fn update(&mut self, price: f64, trade_time_ms: u64) {
        // 获取当前系统时间，用于判断数据是否过期
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        // 清除过期数据 (从队列头部开始检查)
        // saturating_sub: 防止时间戳回退导致的下溢
        while let Some(front) = self.prices.front() {
            if now_ms.saturating_sub(front.timestamp_ms) > self.expire_threshold_ms {
                self.prices.pop_front();
            } else {
                break;  // 队列按时间排序，遇到未过期的就停止
            }
        }

        // 添加新数据点 (存储对数价格以便后续计算)
        self.prices.push_back(PriceData { 
            ln_price: price.ln(), 
            timestamp_ms: trade_time_ms 
        });

        // 保持窗口大小 (VecDeque 不会自动弹出，需手动维护)
        if self.prices.len() > self.window_size {
            self.prices.pop_front();
        }
    }

    /// 计算当前波动率
    /// 
    /// # 返回
    /// - `VolatilityResult`: 包含年化波动率、原始波动率、时间窗口等信息
    pub fn get_volatility(&self) -> VolatilityResult {
        // 数据不足或过期时返回的防御性结果
        let stale_result = VolatilityResult {
            annualized: self.fallback_volatility, 
            raw_vol: 0.0, 
            dt_secs: 0.0, 
            duration_ms: 0, 
            is_stale: true,
        };

        // 至少需要 2 个数据点才能计算收益率
        if self.prices.len() < 2 { 
            return stale_result; 
        }

        // 检查最新数据是否过期 (市场可能中断)
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let latest_ts = self.prices.back().unwrap().timestamp_ms;
        if now_ms.saturating_sub(latest_ts) > self.stale_threshold_ms {
            println!("⚠️ 警告: 市场行情中断! 上次成交: {}ms 前", now_ms - latest_ts);
            return stale_result;
        }

        // 提取所有对数价格
        let ln_prices: Vec<f64> = self.prices.iter().map(|p| p.ln_price).collect();
        let count = ln_prices.len() - 1;  // 收益率数量 = 价格数量 - 1
        
        // 计算对数收益率的平方和
        // windows(2): 滑动窗口，每次取相邻两个元素
        let diff_sq_sum: f64 = ln_prices
            .windows(2)
            .map(|w| (w[1] - w[0]).powi(2))  // powi(2): 整数次幂，比 powf 快
            .sum();
        
        // RMS (均方根) 波动率
        let raw_vol = if count > 0 { 
            (diff_sq_sum / count as f64).sqrt() 
        } else { 
            0.0 
        };

        // 计算时间窗口长度
        let first_ts = self.prices.front().unwrap().timestamp_ms;
        let duration_ms = latest_ts.saturating_sub(first_ts);
        let dt_secs = duration_ms as f64 / 1000.0;
        
        // 年化波动率 = raw_vol * sqrt(年秒数 / 窗口秒数)
        // max(0.01): 防止除零
        let annualized = raw_vol * (self.seconds_in_year / dt_secs.max(0.01)).sqrt();

        VolatilityResult { 
            annualized, 
            raw_vol, 
            dt_secs, 
            duration_ms, 
            is_stale: false 
        }
    }

    /// 检查是否有足够数据进行可靠计算
    pub fn is_ready(&self) -> bool { 
        self.prices.len() >= self.window_size 
    }
    
    /// 检查是否可以进行基本计算 (至少 2 个数据点)
    pub fn can_calculate(&self) -> bool { 
        self.prices.len() >= 2 
    }
}