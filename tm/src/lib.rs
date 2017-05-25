extern crate concurrent;

use std::sync::atomic::{self, AtomicPtr};
use std::mem;

const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;

struct Memory<T> {
    inner: concurrent::Option<T>,
}

impl<T> Memory<T> {
    fn new(data: Option<Box<T>>) -> Memory<T> {
        Memory {
            inner: concurrent::Option::new(data),
        }
    }

    fn with<F>(&self, f: F)
    where
        F: Fn(Option<concurrent::Guard<T>>) -> Option<Box<T>>,
        T: 'static,
    {
        loop {
            let snapshot = self.inner.load(ORDERING);
            let snapshot_ptr = snapshot.as_ref().map(concurrent::Guard::as_raw);
            let ret = f(snapshot);

            if self.inner.compare_and_store(snapshot_ptr, ret, ORDERING).is_ok() {
                break;
            }
        }
    }
}
