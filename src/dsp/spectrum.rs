// 用来传递数据，瓶颈在这里？

use std::sync::{Arc, Mutex};

pub const BANDS: usize = 64;

#[derive(Clone)]
pub struct SharedPipe {
    pub data: Arc<Mutex<Vec<f32>>>,
}

impl SharedPipe {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(vec![0.0; BANDS])),
        }
    }
    pub fn write(&self, new_data: &[f32]) {
        if let Ok(mut guard) = self.data.lock() {
            guard.copy_from_slice(new_data);
        }
    }
    pub fn read(&self) -> Vec<f32> {
        self.data.lock().map(|g| g.clone()).unwrap()
    }
}
