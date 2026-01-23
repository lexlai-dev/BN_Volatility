use crate::common::ring_buffer::RingBuffer;

pub trait TrailingIndicator {
    fn calculate_indicator(&self) -> f64;

    fn processing_calculation(&self) -> f64;

    fn add_sample(&mut self, value: f64, timestamp: f64);
}

pub struct BaseIndicator {
    pub sampling_buffer: RingBuffer,
    pub processing_buffer: RingBuffer,
    pub time_buffer: RingBuffer,
}

impl BaseIndicator {
    pub fn new(sampling_length: usize, processing_length: usize) -> Self {
        Self {
            sampling_buffer: RingBuffer::new(sampling_length),
            processing_buffer: RingBuffer::new(processing_length),
            time_buffer: RingBuffer::new(sampling_length),
        }
    }


    pub fn is_sampling_buffer_full(&self) -> bool {
        self.sampling_buffer.is_full()
    }
}