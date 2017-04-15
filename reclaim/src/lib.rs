mod treiber;
mod rt;

use std::sync::atomic::{self, AtomicPtr};
use std::ops;

struct Snapshot<'a, T> {
    link: rt::Link,
    _marker: PhantomData<'a>,
}

impl<'a, T> ops::Deref for Snapshot<'a, T> {
    fn deref(&self) -> &T {
        &*self.ptr
    }
}

impl<'a, T> Drop for Reader<'a, T> {
    fn drop(&mut self) {
        self.link.set_inactive();
    }
}

impl rt::Linking {
    pub fn load<T>(&self, a: &Atomic<T>) -> Snapshot<T> {
        // Construct a link to the inner pointer.
        let link = rt::Link::new(a.load(atomic::Ordering::Relaxed));

        // Ensure that the object isn't destroyed while a RAII reader exists.
        self.link(link);

        Snapshot {
            link: link,
            _marker: PhantomData,
        }
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
        rt::linking(|r| r.load(self))
    }

    pub fn store(&self, new: Box<T>) {
        // Replace the inner by the new value.
        let old = self.inner.swap(Box::into_raw(new), atomic::Ordering::Relaxed);
        // The old value is now unreachable, so we must mark it so.
        rt::set_unreachable(old);
    }
}
