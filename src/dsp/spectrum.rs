use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub const BANDS: usize = 64;

#[derive(Clone)]
pub struct SharedPipe {
    data: Arc<[Mutex<Vec<f32>>; 2]>, // 双缓冲
    current: Arc<AtomicUsize>,       // 当前读取的缓冲区索引
    version: Arc<AtomicUsize>,       // 数据版本号，用于检测是否有新数据
}

impl SharedPipe {
    pub fn new() -> Self {
        Self {
            data: Arc::new([Mutex::new(vec![0.0; BANDS]), Mutex::new(vec![0.0; BANDS])]),
            current: Arc::new(AtomicUsize::new(0)),
            version: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn write(&self, new_data: &[f32]) {
        // 计算要写入的缓冲区索引（与当前读取的相反）
        let read_idx = self.current.load(Ordering::Acquire);
        let write_idx = (read_idx + 1) % 2;

        // 获取锁并写入数据
        if let Ok(mut guard) = self.data[write_idx].lock() {
            guard.copy_from_slice(new_data);

            // 原子性地切换当前读取的缓冲区
            self.current.store(write_idx, Ordering::Release);

            // 增加版本号，表示有新数据
            self.version.fetch_add(1, Ordering::Release);
        }
    }

    pub fn read(&self) -> Vec<f32> {
        // 获取当前读取的缓冲区索引
        let idx = self.current.load(Ordering::Acquire);

        // 读取数据
        self.data[idx]
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| vec![0.0; BANDS])
    }

    // 新增：非阻塞读取，返回是否有新数据
    pub fn read_if_new(&self) -> Option<Vec<f32>> {
        thread_local! {
            static THREAD_LOCAL_VERSION: AtomicUsize = AtomicUsize::new(0);
        }

        // 使用线程局部存储跟踪每个线程上次读取的版本
        THREAD_LOCAL_VERSION.with(|local_version| {
            let current_version = self.version.load(Ordering::Acquire);
            let last_version = local_version.load(Ordering::Relaxed);

            if current_version > last_version {
                local_version.store(current_version, Ordering::Relaxed);
                Some(self.read())
            } else {
                None
            }
        })
    }

    // 新增：检查是否有新数据（无锁）
    pub fn has_new_data(&self) -> bool {
        thread_local! {
            static THREAD_LOCAL_VERSION: AtomicUsize = AtomicUsize::new(0);
        }

        THREAD_LOCAL_VERSION.with(|local_version| {
            let current_version = self.version.load(Ordering::Acquire);
            let last_version = local_version.load(Ordering::Relaxed);
            current_version > last_version
        })
    }

    // 新增：带版本跟踪的读取
    pub fn read_with_tracking(&self) -> (Vec<f32>, usize) {
        let data = self.read();
        let version = self.version.load(Ordering::Acquire);
        (data, version)
    }
}
