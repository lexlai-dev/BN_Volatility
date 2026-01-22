use crate::common::ring_buffer::RingBuffer;

/// 定义指标必须实现的算法行为 (等同于 Python 的 @abstractmethod)
pub trait TrailingIndicator {
    /// 核心指标计算逻辑
    fn calculate_indicator(&self) -> f64;

    /// 对处理结果的后续加工 (默认行为是求均值)
    fn processing_calculation(&self) -> f64;

    /// 添加新样本的外部接口
    fn add_sample(&mut self, value: f64, timestamp: f64);
}

/// 存放指标共有的缓冲区数据
pub struct BaseIndicator {
    pub sampling_buffer: RingBuffer,
    pub processing_buffer: RingBuffer,
    pub time_buffer: RingBuffer,
    // samples_length: usize,
}

impl BaseIndicator {
    pub fn new(sampling_length: usize, processing_length: usize) -> Self {
        Self {
            sampling_buffer: RingBuffer::new(sampling_length),
            processing_buffer: RingBuffer::new(processing_length),
            time_buffer: RingBuffer::new(sampling_length),
            // samples_length: 0,
        }
    }

    // --- 对应 Python 的 @property 属性 ---

    pub fn is_sampling_buffer_full(&self) -> bool {
        self.sampling_buffer.is_full()
    }

    // pub fn is_processing_buffer_full(&self) -> bool {
    //     self.processing_buffer.is_full()
    // }
    //
    // pub fn sampling_length(&self) -> usize {
    //     self.sampling_buffer.len()
    // }
    //
    // /// 对应 Python 的 is_sampling_buffer_changed
    // pub fn has_sampling_buffer_changed(&mut self) -> bool {
    //     let current_len = self.sampling_buffer.len();
    //     let is_changed = self.samples_length != current_len;
    //     self.samples_length = current_len;
    //     is_changed
    // }
}