use super::base::{BaseIndicator, TrailingIndicator};

/// Calculates Instant Volatility (Realized Volatility) over a sliding window.
/// This implementation typically expects log-prices as input to calculate the
/// standard deviation of log-returns correctly.
pub struct InstantVolatilityIndicator {
    pub base: BaseIndicator,
    seconds_in_year: f64,
}

impl InstantVolatilityIndicator {
    pub fn new(sampling_len: usize, processing_len: usize) -> Self {
        Self {
            base: BaseIndicator::new(sampling_len, processing_len),
            // Constant: 365 * 24 * 60 * 60
            seconds_in_year: 31536000.0,
        }
    }

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

        // Algorithm: Root Mean Square (RMS) of differences.
        // Equivalent to: sqrt( sum((p_i - p_{i-1})^2) / N )
        let diff_sq_sum: f64 = prices.windows(2)
            .map(|w| (w[1] - w[0]).powi(2))
            .sum();

        let raw_vol = (diff_sq_sum / prices.len() as f64).sqrt();

        // Annualization Logic
        let times = self.base.time_buffer.get_as_vec();
        let dt = times[times.len() - 1] - times[0];

        // Avoid division by zero for extremely small time deltas
        if dt < 0.001 {
            0.0
        } else {
            // Scale volatility to an annual basis: raw_vol * sqrt(Year / TimeDelta)
            raw_vol * (self.seconds_in_year / dt).sqrt()
        }
    }

    fn processing_calculation(&self) -> f64 {
        self.base.processing_buffer.get_last_value()
    }

    fn add_sample(&mut self, value: f64, timestamp: f64) {
        self.base.sampling_buffer.add_value(value);
        self.base.time_buffer.add_value(timestamp);

        // Calculate and store the new indicator value
        let indicator_value = self.calculate_indicator();
        self.base.processing_buffer.add_value(indicator_value);
    }
}