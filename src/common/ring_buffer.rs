use std::collections::VecDeque;

pub struct RingBuffer {
    data: VecDeque<f64>,
    capacity: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn add_value(&mut self, val: f64) {
        if self.data.len() >= self.capacity {
            self.data.pop_front();
        }
        self.data.push_back(val);
    }

    pub fn get_as_vec(&self) -> Vec<f64> {
        self.data.iter().cloned().collect()
    }

    pub fn get_last_value(&self) -> f64 {
        *self.data.back().unwrap_or(&f64::NAN)
    }

    pub fn is_full(&self) -> bool {
        self.data.len() == self.capacity
    }
}