use std::sync::Arc;

use parking_lot_rt::{Condvar, Mutex};

pub struct Semaphore {
    inner: Arc<SemaphoreInner>,
}

impl Semaphore {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: SemaphoreInner {
                permissions: <_>::default(),
                capacity,
                cv: Condvar::new(),
            }
            .into(),
        }
    }
    pub fn acquire(&self) -> SemaphoreGuard {
        let mut count = self.inner.permissions.lock();
        while *count == self.inner.capacity {
            self.inner.cv.wait(&mut count);
        }
        *count += 1;
        SemaphoreGuard {
            inner: self.inner.clone(),
        }
    }
}

struct SemaphoreInner {
    permissions: Mutex<usize>,
    capacity: usize,
    cv: Condvar,
}

impl SemaphoreInner {
    fn release(&self) {
        let mut count = self.permissions.lock();
        *count -= 1;
        self.cv.notify_one();
    }
}

#[allow(clippy::module_name_repetitions)]
pub struct SemaphoreGuard {
    inner: Arc<SemaphoreInner>,
}

impl Drop for SemaphoreGuard {
    fn drop(&mut self) {
        self.inner.release();
    }
}
