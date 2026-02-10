//! 指标计算模块
//!
//! - `vol`: 瞬时波动率计算
//! - `calculators`: VWAP、OFI、价格拟合
//! - `trend_state`: 趋势状态机
//! - `base`: 基础指标 trait

pub mod base;
pub mod vol;
pub mod calculators;
pub mod trend_state;