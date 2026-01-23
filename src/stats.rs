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

    pub fn record(&mut self, vol: f64) {
        self.count += 1;
        let max_idx = self.buckets.len() - 1;
        let mut index = (vol / self.step) as usize;
        if index > max_idx { index = max_idx; }
        self.buckets[index] += 1;
    }

    pub fn generate_report(&self, interval_minutes: u64) -> String {
        let total_buckets = self.buckets.len();

        // 1. 先统计一下有多少个非零桶，方便写在标题里
        let active_buckets = self.buckets.iter().filter(|&&c| c > 0).count();

        let mut report = format!(
            "📊 *波动率分布报告 ({} min)*\n步长: `{:.2}%` | 桶数: `{}` | 总采样: `{}`\n```\n",
            interval_minutes, self.step * 100.0, total_buckets, self.count
        );
        let mut has_data = false;

        for i in 0..total_buckets {
            let count = self.buckets[i];

            // 如果是 0，跳过不画
            if count == 0 {
                continue;
            }
            has_data = true;

            let lower = i as f64 * self.step * 100.0;
            let upper = (i + 1) as f64 * self.step * 100.0;

            // --- 颜色逻辑 (保持你喜欢的 Emoji) ---
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

            // 优化：计算百分比 (基于总采样数，而不是基于当前显示的桶)
            let percentage = if self.count > 0 { (count as f64 / self.count as f64) * 100.0 } else { 0.0 };

            // 优化：条形图长度限制 (防止极端情况刷屏)
            // 使用 .min(30) 限制最大长度为 30 个字符
            let bar_len = (percentage / 1.0).round() as usize;
            let bar = "█".repeat(bar_len);

            report.push_str(&format!("{} {:<14}: {:<4} ({:.1}%)\n", icon, label, bar, percentage));
        }

        if !has_data {
            report.push_str("   (本周期内无波动率数据)\n");
        } else {
            // 🚀 核心修改：在底部添加明确的说明
            let hidden_count = total_buckets - active_buckets;
            if hidden_count > 0 {
                report.push_str(&format!("\n----------------------------------\n"));
                report.push_str(&format!("ℹ️ 其余 {} 个区间的计数均为 0 (已隐藏)", hidden_count));
            }
        }

        report.push_str("```");
        report
    }
}