extern crate concurrent;

use std::sync::atomic;

pub struct Memory<T> {
    inner: concurrent::Option<T>,
}

impl<T> Memory<T> {
    pub fn new(data: Option<Box<T>>) -> Memory<T> {
        Memory {
            inner: concurrent::Option::new(data),
        }
    }

    pub fn with<F>(&self, f: F)
    where
        F: Fn(Option<concurrent::Guard<T>>) -> Option<Box<T>>,
        T: 'static,
    {
        loop {
            let snapshot = self.inner.load(atomic::Ordering::Relaxed);
            let snapshot_ptr = snapshot.as_ref().map(concurrent::Guard::as_raw);
            let ret = f(snapshot);

            if self.inner.compare_and_store(snapshot_ptr, ret, atomic::Ordering::Relaxed).is_ok() {
                break;
            }
        }
    }
}
