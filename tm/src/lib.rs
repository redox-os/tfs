use std::sync::atomic::{self, AtomicPtr};
use std::mem;

const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;

struct Memory<T> {
    inner: concurrent::Option<T>,
}

impl<T> Memory<T> {
    fn new(data: T) -> Memory<T> {
        Memory {
            inner: concurrent::Option::new(Some(Box::new(data))),
        }
    }

    fn with<F>(&self, f: F)
    where F: Fn(&T) -> T {
        loop {
            let snapshot = self.inner.load(ORDERING).unwrap();
            let ret = f(&*snapshot);

            if self.inner.compare_and_store(Some(&*snapshot), Some(ret), ORDERING).is_ok() {
                break;
            }
        }
    }
}
