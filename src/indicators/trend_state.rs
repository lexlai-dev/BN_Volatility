//! 趋势状态机模块
//!
//! 基于价格拟合斜率和 OFI 判断趋势方向，管理信号状态。

use std::collections::VecDeque;

use super::calculators::FitResult;

/// 趋势方向
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrendDirection {
    Long = 1,    // 看涨
    Short = -1,  // 看跌
    Neutral = 0, // 中性
}

/// 策略状态
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrategyState {
    Cooldown = -1, // 冷却期（退出后等待）
    Scanning = 0,  // 扫描中（寻找入场信号）
    Holding = 1,   // 持仓中（监控退出条件）
}

pub struct TrendStateMachine {
    state: StrategyState,
    direction: TrendDirection,
    
    // 入场参数
    entry_slope: f64,
    entry_intercept: f64,
    entry_ts_sec: f64,
    
    // 冷却期
    cooldown_start_ts: f64,
    cooldown_secs: f64,
    
    // 阈值参数
    slope_threshold: f64,
    ofi_confirm_threshold: f64,
    
    // 退出参数
    slope_threshold_ratio: f64,
    min_price_fallback: f64,
    max_price_fallback: f64,
    entry_protection_secs: f64,
    
    // 斜率历史（用于斜率反转退出）
    slope_history: VecDeque<f64>,
    slope_weak_threshold: f64,
}

#[derive(Debug, Clone)]
pub struct TrendConfig {
    pub slope_threshold: f64,
    pub ofi_confirm_threshold: f64,
    pub cooldown_secs: f64,
    pub slope_threshold_ratio: f64,
    pub min_price_fallback: f64,
    pub max_price_fallback: f64,
    pub entry_protection_secs: f64,
    pub slope_weak_threshold: f64,
}

impl TrendStateMachine {
    pub fn new(config: TrendConfig) -> Self {
        Self {
            state: StrategyState::Scanning,
            direction: TrendDirection::Neutral,
            entry_slope: 0.0,
            entry_intercept: 0.0,
            entry_ts_sec: 0.0,
            cooldown_start_ts: 0.0,
            cooldown_secs: config.cooldown_secs,
            slope_threshold: config.slope_threshold,
            ofi_confirm_threshold: config.ofi_confirm_threshold,
            slope_threshold_ratio: config.slope_threshold_ratio,
            min_price_fallback: config.min_price_fallback,
            max_price_fallback: config.max_price_fallback,
            entry_protection_secs: config.entry_protection_secs,
            slope_history: VecDeque::with_capacity(10),
            slope_weak_threshold: config.slope_weak_threshold,
        }
    }

    /// 更新状态机
    /// 
    /// 根据拟合结果和 OFI 更新趋势方向。
    /// 调用者通过 `get_direction()` 获取当前趋势。
    pub fn update(
        &mut self,
        current_ts_sec: f64,
        fit_5s: Option<&FitResult>,
        cum_ofi: f64,
        latest_price: f64,
    ) {
        match self.state {
            StrategyState::Cooldown => {
                // 冷却期结束后恢复扫描
                if current_ts_sec - self.cooldown_start_ts >= self.cooldown_secs {
                    self.state = StrategyState::Scanning;
                }
            }

            StrategyState::Scanning => {
                let fit = match fit_5s {
                    Some(f) if f.is_valid => f,
                    _ => return,
                };

                // 多头信号: slope > threshold && ofi > confirm_threshold
                if fit.slope > self.slope_threshold && cum_ofi > self.ofi_confirm_threshold {
                    self.enter_position(TrendDirection::Long, fit, current_ts_sec);
                }
                // 空头信号: slope < -threshold && ofi < -confirm_threshold
                else if fit.slope < -self.slope_threshold && cum_ofi < -self.ofi_confirm_threshold {
                    self.enter_position(TrendDirection::Short, fit, current_ts_sec);
                }
            }

            StrategyState::Holding => {
                let fit = match fit_5s {
                    Some(f) => f,
                    None => return,
                };

                // 记录斜率历史
                self.slope_history.push_back(fit.slope);
                if self.slope_history.len() > 10 {
                    self.slope_history.pop_front();
                }

                let time_elapsed = current_ts_sec - self.entry_ts_sec;
                
                // 检查退出条件（入场保护期后）
                if time_elapsed >= self.entry_protection_secs {
                    let fitted_price = self.entry_intercept + self.entry_slope * time_elapsed;
                    let raw_threshold = (1.0 - self.slope_threshold_ratio) * self.entry_slope.abs() * time_elapsed;
                    let threshold = raw_threshold.clamp(self.min_price_fallback, self.max_price_fallback);

                    let should_exit = match self.direction {
                        TrendDirection::Long => latest_price < fitted_price - threshold,
                        TrendDirection::Short => latest_price > fitted_price + threshold,
                        TrendDirection::Neutral => false,
                    };

                    if should_exit {
                        self.exit_position(current_ts_sec);
                        return;
                    }
                }

                // 斜率反转退出
                if time_elapsed >= 5.0 && self.slope_history.len() >= 10 {
                    let weak_count = match self.direction {
                        TrendDirection::Long => self.slope_history.iter().filter(|&&s| s < self.slope_weak_threshold).count(),
                        TrendDirection::Short => self.slope_history.iter().filter(|&&s| s > -self.slope_weak_threshold).count(),
                        TrendDirection::Neutral => 0,
                    };

                    if weak_count > 5 {
                        self.exit_position(current_ts_sec);
                    }
                }
            }
        }
    }

    fn enter_position(&mut self, direction: TrendDirection, fit: &FitResult, ts_sec: f64) {
        self.state = StrategyState::Holding;
        self.direction = direction;
        self.entry_slope = fit.slope;
        self.entry_intercept = fit.current_price;
        self.entry_ts_sec = ts_sec;
        self.slope_history.clear();
    }

    fn exit_position(&mut self, ts_sec: f64) {
        self.state = StrategyState::Cooldown;
        self.cooldown_start_ts = ts_sec;
        self.direction = TrendDirection::Neutral;
        self.slope_history.clear();
    }

    pub fn get_state(&self) -> StrategyState {
        self.state
    }

    pub fn get_direction(&self) -> TrendDirection {
        self.direction
    }

    pub fn is_holding(&self) -> bool {
        self.state == StrategyState::Holding
    }
}
