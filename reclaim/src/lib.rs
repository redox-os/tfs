mod treiber;
mod rt;

use std::sync::atomic::{self, AtomicPtr};

struct Snapshot<'a, T> {
    raw: RawSnapshot,
    _marker: PhantomData<'a>,
}

impl<'a, T> Snapshot<'a, T> {
    fn drop(&mut self) {
        self.raw.active.store(true);
    }
}

pub struct Atomic<T> {
    inner: AtomicPtr<T>,
}

impl<T> Atomic<T> {
    pub fn new(inner: T) -> Atomic<T> {
        Atomic {
            inner: AtomicPtr::new(Box::into_raw(Box::new(inner))),
        }
    }

    pub fn load(&self) -> Snapshot<T> {
        read(|r| r.load(self))
    }

    pub fn store(&self, new: Box<T>) {
        // Replace the inner by the new value.
        let old = self.inner.swap(Box::into_raw(new), atomic::Ordering::Relaxed);
        // Push the old pointer to the garbage stack.
        GARBAGE.push(Box::from_raw(old));

        tick();
    }
}
