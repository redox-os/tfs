use std::sync::atomic::{self, AtomicPtr};
use std::mem;

const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;

struct Memory<T> {
    inner: AtomicPtr<T>,
}

impl<T> Memory<T> {
    fn new(data: T) -> Memory<T> {
        Memory {
            inner: AtomicPtr::new(Box::into_raw(Box::new(data))),
        }
    }

    fn with<U, F>(&self, f: F)
    where F: Fn(&mut T) -> U {
        loop {
            let ptr = unsafe { &mut *self.inner.load(ORDERING) };

            let ret = Box::new(f(ptr));
            if self.inner.compare_and_swap(ptr, &mut ret, ORDERING) == ptr {
                mem::forget(ret);
                break;
            }
        }
    }
}
