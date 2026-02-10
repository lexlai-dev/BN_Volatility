//! 趋势计算器模块
//!
//! 包含三个核心计算器：
//! - `VwapCalculator`: VWAP (成交量加权平均价) 计算
//! - `DepthCalculator`: 订单簿深度计算器 (OFI + 冲击价格)
//! - `PriceFitter`: 价格线性拟合

use std::collections::{HashMap, VecDeque};

// ============================================================================
// VWAP 计算器
// ============================================================================

/// VWAP 计算器：按时间窗口聚合 aggTrade，计算成交量加权平均价
/// 
/// # 算法原理 (增量累加)
/// VWAP = Σ(price × qty) / Σ(qty)
/// 
/// 不缓存每笔交易，而是直接累加 sum_pq 和 sum_q，窗口结束时一次除法得出 VWAP。
/// 这样可以节省内存，且计算复杂度为 O(1)。
/// 
/// # 使用方式
/// ```ignore
/// let mut vwap = VwapCalculator::new(100, 1000);  // 100ms 窗口, 最多保留 1000 个 VWAP
/// if let Some(point) = vwap.add_trade(price, qty, timestamp_ms) {
///     // 窗口完成，point.price 是这个窗口的 VWAP
/// }
/// ```
pub struct VwapCalculator {
    window_ms: u64,           // 聚合窗口大小 (毫秒)
    window_start_ms: u64,     // 当前窗口开始时间
    
    // 增量累加字段
    sum_pq: f64,              // Σ(price × qty) - 价格×数量的累加和
    sum_q: f64,               // Σ(qty) - 数量的累加和
    last_ts_ms: u64,          // 最后一笔交易的时间戳
    
    // VWAP 序列 (用于后续的价格拟合)
    vwap_series: VecDeque<VwapPoint>,
    max_series_len: usize,    // 序列最大长度
}

/// VWAP 数据点
#[derive(Clone, Copy)]
pub struct VwapPoint {
    pub price: f64,           // VWAP 价格
    pub timestamp_ms: u64,    // 时间戳
}

impl VwapCalculator {
    pub fn new(window_ms: u64, max_series_len: usize) -> Self {
        Self {
            window_ms,
            window_start_ms: 0,
            sum_pq: 0.0,
            sum_q: 0.0,
            last_ts_ms: 0,
            vwap_series: VecDeque::with_capacity(max_series_len),
            max_series_len,
        }
    }

    /// 添加一笔交易，返回 Some(vwap) 如果窗口完成
    pub fn add_trade(&mut self, price: f64, qty: f64, timestamp_ms: u64) -> Option<VwapPoint> {
        if self.window_start_ms == 0 {
            self.window_start_ms = timestamp_ms;
            self.sum_pq = price * qty;
            self.sum_q = qty;
            self.last_ts_ms = timestamp_ms;
            return None;
        }

        if timestamp_ms - self.window_start_ms < self.window_ms {
            // 增量累加
            self.sum_pq += price * qty;
            self.sum_q += qty;
            self.last_ts_ms = timestamp_ms;
            return None;
        }

        // 窗口完成，计算 VWAP
        let vwap_point = self.flush();
        
        // 开始新窗口
        self.window_start_ms = timestamp_ms;
        self.sum_pq = price * qty;
        self.sum_q = qty;
        self.last_ts_ms = timestamp_ms;

        vwap_point
    }

    fn flush(&mut self) -> Option<VwapPoint> {
        if self.sum_q <= 0.0 {
            return None;
        }

        let vwap = self.sum_pq / self.sum_q;
        let point = VwapPoint { price: vwap, timestamp_ms: self.last_ts_ms };

        // 添加到序列
        self.vwap_series.push_back(point);
        if self.vwap_series.len() > self.max_series_len {
            self.vwap_series.pop_front();
        }

        Some(point)
    }

    pub fn get_series(&self) -> &VecDeque<VwapPoint> {
        &self.vwap_series
    }

    /// 清理过期数据
    pub fn cleanup(&mut self, cutoff_ms: u64) {
        while let Some(front) = self.vwap_series.front() {
            if front.timestamp_ms < cutoff_ms {
                self.vwap_series.pop_front();
            } else {
                break;
            }
        }
    }
}

// ============================================================================
// OFI 计算器
// ============================================================================

/// OFI (Order Flow Imbalance) 计算器：基于 depth20 计算累积订单流不平衡
/// 
/// # 算法原理
/// OFI 衡量买卖双方挂单的净变化，考虑：
/// 1. 挂单簿的变化 (新增/撤销)
/// 2. 成交消耗的挂单量
/// 3. 距离中间价的衰减权重
/// 
/// 公式: OFI = bid_flow - ask_flow
/// - bid_flow > 0: 买方力量增强 (看涨)
/// - ask_flow > 0: 卖方力量增强 (看跌)
/// 
/// # 累积 OFI
/// 对历史 OFI 进行时间衰减加权累加，形成趋势信号。
pub struct DepthCalculator {
    // 上一次的订单簿快照 (价格转为整数分以避免浮点比较问题)
    prev_bids: HashMap<u64, f64>,  // price (分) -> qty
    prev_asks: HashMap<u64, f64>,
    last_update_id: u64,           // 用于检测数据连续性
    
    // OFI 累积缓冲区
    ofi_buffer: VecDeque<(f64, f64)>,  // (timestamp_sec, raw_ofi)
    cum_window_secs: f64,              // 累积窗口大小 (秒)
    decay: f64,                        // 时间衰减因子 (0-1)
    
    // 成交缓冲区 (用于计算被吃掉的挂单)
    // 格式: (时间戳ms, 价格, 数量, 是否卖单主动成交)
    trade_buffer: VecDeque<(u64, f64, f64, bool)>,
    last_depth_ts_ms: u64,             // 上次深度更新时间
    
    // 冲击价格 (买卖双方各吃 target_qty BTC 的加权平均价的均值)
    impact_price: f64,
    impact_qty: f64,                   // 实际计算使用的数量
}

impl DepthCalculator {
    pub fn new(cum_window_secs: f64, decay: f64) -> Self {
        Self {
            prev_bids: HashMap::new(),
            prev_asks: HashMap::new(),
            last_update_id: 0,
            ofi_buffer: VecDeque::with_capacity(100),
            cum_window_secs,
            decay,
            trade_buffer: VecDeque::with_capacity(1000),
            last_depth_ts_ms: 0,
            impact_price: 0.0,
            impact_qty: 0.0,
        }
    }

    /// 添加成交数据（用于后续 OFI 计算）
    pub fn add_trade(&mut self, timestamp_ms: u64, price: f64, qty: f64, is_buyer_maker: bool) {
        self.trade_buffer.push_back((timestamp_ms, price, qty, is_buyer_maker));
        // 保留最近 10000 笔
        if self.trade_buffer.len() > 10000 {
            self.trade_buffer.pop_front();
        }
    }

    /// 处理深度更新，返回 (raw_ofi, cum_ofi, mid_price)
    pub fn update_depth(
        &mut self,
        update_id: u64,
        trans_time_ms: u64,
        bids: &[(f64, f64)],
        asks: &[(f64, f64)],
    ) -> Option<(f64, f64, f64)> {
        if update_id <= self.last_update_id {
            return None;
        }
        self.last_update_id = update_id;

        // 构建当前订单簿
        let curr_bids: HashMap<u64, f64> = bids.iter()
            .map(|(p, q)| ((*p * 100.0) as u64, *q))
            .collect();
        let curr_asks: HashMap<u64, f64> = asks.iter()
            .map(|(p, q)| ((*p * 100.0) as u64, *q))
            .collect();

        // 计算中间价
        let best_bid = bids.iter().map(|(p, _)| *p).fold(0.0_f64, f64::max);
        let best_ask = asks.iter().map(|(p, _)| *p).fold(f64::MAX, f64::min);
        if best_bid <= 0.0 || best_ask >= f64::MAX {
            self.prev_bids = curr_bids;
            self.prev_asks = curr_asks;
            self.last_depth_ts_ms = trans_time_ms;
            return None;
        }
        let mid_price = (best_bid + best_ask) / 2.0;

        // 如果是第一次，只保存状态
        if self.prev_bids.is_empty() {
            self.prev_bids = curr_bids;
            self.prev_asks = curr_asks;
            self.last_depth_ts_ms = trans_time_ms;
            return None;
        }

        // 提取这段时间内的成交
        let mut slice_bids: HashMap<u64, f64> = HashMap::new();
        let mut slice_asks: HashMap<u64, f64> = HashMap::new();
        let mut trades_to_keep = VecDeque::new();

        for (ts, p, q, is_buyer_maker) in self.trade_buffer.drain(..) {
            if ts <= self.last_depth_ts_ms {
                continue;
            }
            if ts <= trans_time_ms {
                let price_key = (p * 100.0) as u64;
                if is_buyer_maker {
                    *slice_bids.entry(price_key).or_insert(0.0) += q;
                } else {
                    *slice_asks.entry(price_key).or_insert(0.0) += q;
                }
            } else {
                trades_to_keep.push_back((ts, p, q, is_buyer_maker));
            }
        }
        self.trade_buffer = trades_to_keep;

        // 计算净限价单流
        let b_flow = self.calculate_net_limit_flow(&self.prev_bids, &curr_bids, &slice_asks, mid_price);
        let a_flow = self.calculate_net_limit_flow(&self.prev_asks, &curr_asks, &slice_bids, mid_price);
        let raw_ofi = b_flow - a_flow;

        // 更新 OFI 缓冲区
        let ts_sec = trans_time_ms as f64 / 1000.0;
        self.ofi_buffer.push_back((ts_sec, raw_ofi));

        // 清理过期数据
        let cutoff = ts_sec - self.cum_window_secs;
        while let Some((t, _)) = self.ofi_buffer.front() {
            if *t < cutoff {
                self.ofi_buffer.pop_front();
            } else {
                break;
            }
        }

        // 计算累积 OFI（带时间衰减）
        let mut cum_ofi = 0.0;
        for (t, v) in &self.ofi_buffer {
            let age = ts_sec - t;
            let weight = self.decay.powf(age * 10.0);
            cum_ofi += v * weight;
        }

        // 保存当前状态
        self.prev_bids = curr_bids;
        self.prev_asks = curr_asks;
        self.last_depth_ts_ms = trans_time_ms;

        Some((raw_ofi, cum_ofi, mid_price))
    }

    fn calculate_net_limit_flow(
        &self,
        prev_book: &HashMap<u64, f64>,
        curr_book: &HashMap<u64, f64>,
        trades: &HashMap<u64, f64>,
        mid_price: f64,
    ) -> f64 {
        let decay = 0.2;
        let mut flow = 0.0;

        let mut all_prices: std::collections::HashSet<u64> = prev_book.keys().copied().collect();
        all_prices.extend(curr_book.keys());
        all_prices.extend(trades.keys());

        for price_key in all_prices {
            let p = price_key as f64 / 100.0;
            let v_old = prev_book.get(&price_key).copied().unwrap_or(0.0);
            let v_new = curr_book.get(&price_key).copied().unwrap_or(0.0);
            let v_trade = trades.get(&price_key).copied().unwrap_or(0.0);

            let net_change = (v_new - v_old) + v_trade;
            if net_change.abs() > 1e-6 {
                let w = (-decay * (p - mid_price).abs()).exp();
                flow += net_change * w;
            }
        }

        flow
    }

    pub fn get_cum_ofi(&self) -> f64 {
        // 返回最新的累积 OFI
        if let Some((ts_sec, _)) = self.ofi_buffer.back() {
            let mut cum = 0.0;
            for (t, v) in &self.ofi_buffer {
                let age = ts_sec - t;
                let weight = self.decay.powf(age * 10.0);
                cum += v * weight;
            }
            cum
        } else {
            0.0
        }
    }

    /// 计算冲击价格：买卖双方各吃 target_qty BTC 的加权平均价的均值
    /// 
    /// # 算法
    /// 1. 买入冲击价：从 asks 最低价开始扫，累计 target_qty BTC 的加权平均价
    /// 2. 卖出冲击价：从 bids 最高价开始扫，累计 target_qty BTC 的加权平均价
    /// 3. 返回两者均值
    /// 
    /// 如果某一侧深度不足 target_qty，则以较小的量为准
    pub fn calculate_impact_price(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], target_qty: f64) {
        // 计算各侧可用总量
        let bid_total: f64 = bids.iter().map(|(_, q)| q).sum();
        let ask_total: f64 = asks.iter().map(|(_, q)| q).sum();
        
        // 取两侧可用量和目标量的最小值
        let actual_qty = target_qty.min(bid_total).min(ask_total);
        if actual_qty <= 0.0 {
            return;
        }
        
        // 计算买入冲击价（扫 asks，从低到高）
        let mut sorted_asks: Vec<(f64, f64)> = asks.to_vec();
        sorted_asks.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        
        let mut buy_impact = 0.0;
        let mut remaining = actual_qty;
        for (price, qty) in &sorted_asks {
            let take = qty.min(remaining);
            buy_impact += price * take;
            remaining -= take;
            if remaining <= 0.0 {
                break;
            }
        }
        buy_impact /= actual_qty;
        
        // 计算卖出冲击价（扫 bids，从高到低）
        let mut sorted_bids: Vec<(f64, f64)> = bids.to_vec();
        sorted_bids.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        
        let mut sell_impact = 0.0;
        remaining = actual_qty;
        for (price, qty) in &sorted_bids {
            let take = qty.min(remaining);
            sell_impact += price * take;
            remaining -= take;
            if remaining <= 0.0 {
                break;
            }
        }
        sell_impact /= actual_qty;
        
        // 保存结果
        self.impact_price = (buy_impact + sell_impact) / 2.0;
        self.impact_qty = actual_qty;
    }

    /// 获取当前冲击价格
    pub fn get_impact_price(&self) -> f64 {
        self.impact_price
    }
}

/// 价格拟合器：对 VWAP 序列进行线性拟合
pub struct PriceFitter {
    window_secs: f64,
    min_points: usize,
    min_r2: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct FitResult {
    pub slope: f64,           // 斜率 ($/s)
    pub intercept: f64,       // 截距（当前价格）
    pub r_squared: f64,       // 拟合优度
    pub is_valid: bool,       // 是否有效趋势
    pub current_price: f64,   // 拟合线在当前时刻的价格
}

impl PriceFitter {
    pub fn new(window_secs: f64, min_points: usize, min_r2: f64) -> Self {
        Self { window_secs, min_points, min_r2 }
    }

    /// 对 VWAP 序列进行线性拟合
    pub fn fit(&self, series: &VecDeque<VwapPoint>, current_ts_ms: u64) -> Option<FitResult> {
        let current_ts = current_ts_ms as f64 / 1000.0;
        let cutoff = current_ts - self.window_secs;

        // 筛选窗口内的数据
        let points: Vec<(f64, f64)> = series.iter()
            .filter(|p| (p.timestamp_ms as f64 / 1000.0) >= cutoff)
            .map(|p| (p.timestamp_ms as f64 / 1000.0, p.price))
            .collect();

        if points.len() < self.min_points {
            return None;
        }

        // 归一化时间
        let t0 = points[0].0;
        let t_norm: Vec<f64> = points.iter().map(|(t, _)| t - t0).collect();
        let prices: Vec<f64> = points.iter().map(|(_, p)| *p).collect();

        // 线性拟合 (最小二乘法)
        let n = points.len() as f64;
        let sum_t: f64 = t_norm.iter().sum();
        let sum_p: f64 = prices.iter().sum();
        let sum_tt: f64 = t_norm.iter().map(|t| t * t).sum();
        let sum_tp: f64 = t_norm.iter().zip(prices.iter()).map(|(t, p)| t * p).sum();

        let denom = n * sum_tt - sum_t * sum_t;
        if denom.abs() < 1e-10 {
            return None;
        }

        let slope = (n * sum_tp - sum_t * sum_p) / denom;
        let intercept = (sum_p - slope * sum_t) / n;

        // 计算 R²
        let mean_p = sum_p / n;
        let ss_tot: f64 = prices.iter().map(|p| (p - mean_p).powi(2)).sum();
        let ss_res: f64 = t_norm.iter().zip(prices.iter())
            .map(|(t, p)| {
                let pred = intercept + slope * t;
                (p - pred).powi(2)
            })
            .sum();

        let r_squared = if ss_tot > 0.0 { 1.0 - ss_res / ss_tot } else { 0.0 };
        let is_valid = r_squared >= self.min_r2;

        // 计算当前价格（拟合线在最后时刻的值）
        let last_t = t_norm.last().unwrap();
        let current_price = intercept + slope * last_t;

        Some(FitResult {
            slope,
            intercept,
            r_squared,
            is_valid,
            current_price,
        })
    }

    /// 预测未来价格
    pub fn predict(&self, fit: &FitResult, horizon_secs: f64) -> f64 {
        fit.current_price + fit.slope * horizon_secs
    }
}
