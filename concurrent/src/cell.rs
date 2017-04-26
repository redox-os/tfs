use std::mem;
use std::sync::atomic::{self, AtomicPtr};

use local;
use garbage::Garbage;
use guard::Guard;

pub struct Cell<T> {
    inner: AtomicPtr<T>,
}

impl<T> Cell<T> {
    pub fn load(&self, ordering: atomic::Ordering) -> Guard<T> {
        Guard::new(|| unsafe { &*self.inner.load(ordering) });
    }

    pub fn store(&self, new: Box<T>, ordering: atomic::Ordering) {
        local::add_garbage(unsafe { Garbage::new(self.inner.swap(&new, ordering)) });
    }

    pub fn swap(&self, new: Box<T>, ordering: atomic::Ordering) -> Guard<T> {
        let guard = Guard::new(|| {
            unsafe { &*self.inner.swap(new, ordering) }
        });

        local::add_garbage(Garbage::new(&guard));

        guard
    }

    pub fn compare_and_swap(&self, old: &T, new: Box<T>, ordering: atomic::Ordering)
    -> Result<Guard<T>, (Guard<T>, Box<T>)> {
        let guard = Guard::new(|| {
            unsafe { &*self.inner.compare_and_swap(old, &new, ordering) }
        });

        if guard == old {
            // This is critical for avoiding premature drop as the pointer to the box is stored in
            // `self.inner` now.
            mem::forget(new);

            local::add_garbage(Garbage::new(old));

            Ok(guard)
        } else {
            Err((guard, new))
        }
    }
}
