use super::base::{BaseIndicator, TrailingIndicator};

pub struct InstantVolatilityIndicator {
    pub base: BaseIndicator,
    seconds_in_year: f64,
}

impl InstantVolatilityIndicator {
    pub fn new(sampling_len: usize, processing_len: usize) -> Self {
        Self {
            base: BaseIndicator::new(sampling_len, processing_len),
            seconds_in_year: 31536000.0,
        }
    }

    // 代理方法：方便在 main.rs 中直接调用
    pub fn is_sampling_buffer_full(&self) -> bool {
        self.base.is_sampling_buffer_full()
    }

    pub fn current_value(&self) -> f64 {
        self.processing_calculation()
    }
}

impl TrailingIndicator for InstantVolatilityIndicator {
    fn calculate_indicator(&self) -> f64 {
        let prices = self.base.sampling_buffer.get_as_vec();
        if prices.len() < 2 { return 0.0; }

        // 算法：np.sqrt(np.sum(np.square(np.diff(prices))) / size)
        let diff_sq_sum: f64 = prices.windows(2)
            .map(|w| (w[1] - w[0]).powi(2))
            .sum();

        let raw_vol = (diff_sq_sum / prices.len() as f64).sqrt();

        // 年化逻辑
        let times = self.base.time_buffer.get_as_vec();
        let dt = times[times.len() - 1] - times[0];

        if dt < 0.001 { 0.0 } else {
            raw_vol * (self.seconds_in_year / dt).sqrt()
        }
    }

    fn processing_calculation(&self) -> f64 {
        // 对应 Python: return self._processing_buffer.get_last_value()
        self.base.processing_buffer.get_last_value()
    }

    fn add_sample(&mut self, value: f64, timestamp: f64) {
        self.base.sampling_buffer.add_value(value);
        self.base.time_buffer.add_value(timestamp);

        // 调用算法计算
        let indicator_value = self.calculate_indicator();
        self.base.processing_buffer.add_value(indicator_value);
    }
}