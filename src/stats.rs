pub struct VolatilityStats {
    pub buckets: Vec<usize>,
    pub count: u32,
    pub step: f64,
}

impl VolatilityStats {
    pub fn new(step: f64, bucket_count: usize) -> Self {
        Self {
            buckets: vec![0; bucket_count],
            count: 0,
            step,
        }
    }

    /// Records a new volatility sample into the appropriate bucket.
    pub fn record(&mut self, vol: f64) {
        self.count += 1;
        let max_idx = self.buckets.len() - 1;

        // Calculate bucket index based on step size.
        let mut index = (vol / self.step) as usize;

        // Clamp index to the last bucket if volatility exceeds the max range.
        if index > max_idx {
            index = max_idx;
        }

        self.buckets[index] += 1;
    }

    /// Generates a formatted ASCII histogram report for Slack.
    /// Uses a sparse approach (skips empty buckets) to keep the message concise.
    pub fn generate_report(&self, interval_minutes: u64) -> String {
        let total_buckets = self.buckets.len();

        // Count non-zero buckets to display in the header.
        let active_buckets = self.buckets.iter().filter(|&&c| c > 0).count();

        let mut report = format!(
            "📊 *Volatility Distribution ({} min)*\nStep: `{:.2}%` | Total Samples: `{}`\n```\n",
            interval_minutes, self.step * 100.0, self.count
        );
        let mut has_data = false;

        for i in 0..total_buckets {
            let count = self.buckets[i];

            // Skip buckets with no data to save vertical space.
            if count == 0 {
                continue;
            }
            has_data = true;

            let lower = i as f64 * self.step * 100.0;
            let upper = (i + 1) as f64 * self.step * 100.0;

            // --- Color Logic (Emoji Heatmap) ---
            // 0-20%: Low (Blue)
            // 20-60%: Medium (Yellow)
            // 60-90%: High (Red)
            // >90%: Extreme (Fire)
            let progress = i as f64 / total_buckets as f64;
            let icon = if progress < 0.2 { "🔵" }
            else if progress < 0.6 { "🟡" }
            else if progress < 0.9 { "🔴" }
            else { "🔥" };

            let label = if i < (total_buckets - 1) {
                format!("{:.2}-{:.2}%", lower, upper)
            } else {
                format!("{:.2}%+", lower)
            };

            // Calculate percentage based on total samples.
            let percentage = if self.count > 0 { (count as f64 / self.count as f64) * 100.0 } else { 0.0 };

            // Generate bar chart.
            // Scale: 1 character per 1% to accommodate high-resolution histograms.
            let bar_len = (percentage / 1.0).round() as usize;
            let bar = "█".repeat(bar_len);

            report.push_str(&format!("{} {:<14}: {:<4} ({:.1}%)\n", icon, label, bar, percentage));
        }

        if !has_data {
            report.push_str("   (No volatility data recorded in this interval)\n");
        } else {
            // Footer: Explicitly mention hidden zero-count buckets to avoid ambiguity.
            let hidden_count = total_buckets - active_buckets;
            if hidden_count > 0 {
                report.push_str("\n----------------------------------\n");
                report.push_str(&format!("ℹ️ {} empty buckets hidden", hidden_count));
            }
        }

        report.push_str("```");
        report
    }
}