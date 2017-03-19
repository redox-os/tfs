#![feature(conservative_impl_trait, fnbox)]

extern crate crossbeam;
extern crate futures;

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::boxed::FnBox;
use std::sync::atomic::{self, AtomicBool};

use crossbeam::sync::{AtomicOption, SegQueue};
use futures::Future;
use futures::task::{self, Task};

const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;

/// A `Lutex` worker future.
///
/// This future does on job queued to the lutex every poll. When the job of the data, it
/// represents, is done, the return value of the inner data's job is returned.
pub struct Worker<T, R, E> {
    ret: Arc<AtomicOption<Result<R, E>>>,
    lutex: Arc<LutexInternal<T>>,
}

impl<T, R, E> Future for Worker<T, R, E> {
    type Item = R;
    type Error = E;

    fn poll(&mut self) -> futures::Poll<R, E> {
        self.lutex.progress();

        match self.ret.take(ORDERING) {
            Some(Ok(x)) => Ok(futures::Async::Ready(x)),
            Some(Err(x)) => Err(x),
            None => {
                self.lutex.park();
                Ok(futures::Async::NotReady)
            },
        }
    }
}

struct LutexInternal<T> {
    data: UnsafeCell<T>,
    locked: AtomicBool,
    queue: SegQueue<Box<FnBox(*mut T)>>,
    parked: SegQueue<Task>,
}

impl<T> LutexInternal<T> {
    fn new(data: T) -> LutexInternal<T> {
        LutexInternal {
            data: UnsafeCell::new(data),
            locked: AtomicBool::new(false),
            queue: SegQueue::new(),
            parked: SegQueue::new(),
        }
    }

    fn progress(&self) {
        if !self.locked.swap(true, ORDERING) {
            let f = self.queue.try_pop().unwrap();
            f(self.data.get());
            self.locked.store(false, ORDERING);
            self.parked.try_pop().map(|task| task.unpark());
        }
    }

    fn park(&self) {
        self.parked.push(task::park());
    }
}

unsafe impl<T> Sync for LutexInternal<T> {}
unsafe impl<T> Send for LutexInternal<T> {}

pub struct Lutex<T> {
    inner: Arc<LutexInternal<T>>,
}

impl<T> Lutex<T> {
    /// Create a new `Lutex<T>` with some initial data.
    pub fn new(data: T) -> Lutex<T> {
        Lutex {
            inner: Arc::new(LutexInternal::new(data)),
        }
    }

    pub fn with<R: 'static, F: 'static + Send>(&self, f: F) -> Worker<T, R, ()>
        where F: FnOnce(&mut T) -> R {
        self.try_with::<R, (), _>(|x| {
            Ok(f(x))
        })
    }

    pub fn try_with<R: 'static, E: 'static, F: 'static + Send>(&self, f: F) -> Worker<T, R, E>
        where F: FnOnce(&mut T) -> Result<R, E> {
        let ret = Arc::new(AtomicOption::new());

        let ret2 = ret.clone();
        self.inner.queue.push(Box::new(move |x: *mut T| {
            ret2.swap(f(unsafe { &mut *x }), ORDERING);
        }));

        Worker {
            ret: ret,
            lutex: self.inner.clone(),
        }
    }
}

impl<T> Clone for Lutex<T> {
    fn clone(&self) -> Lutex<T> {
        Lutex {
            inner: self.inner.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn single_thread_queue() {
        let lutex = Lutex::new(200);

        lutex.with(|x| *x += 2).wait().unwrap();
        lutex.with(|x| assert_eq!(*x, 202)).wait().unwrap();
        lutex.with(|x| *x = 100).wait().unwrap();
        lutex.with(|x| *x += 2).wait().unwrap();
        lutex.with(|x| *x += 2).wait().unwrap();
        lutex.with(|x| assert_eq!(*x, 104)).wait().unwrap();
    }

    #[test]
    fn many_threads() {
        let lutex = Lutex::new(0);
        let mut v = Vec::new();
        for _ in 0..1000 {
            let lutex = lutex.clone();
            v.push(thread::spawn(move || lutex.with(|x| *x += 1)));
        }

        v.push(thread::spawn(move || lutex.with(|x| assert_eq!(*x, 100))));

        for i in v {
            i.join().unwrap();
        }
    }

    #[test]
    fn mutually_exclusive() {
        let lutex = Lutex::new(false);
        let mut v = Vec::new();
        for _ in 0..1000 {
            let lutex = lutex.clone();
            v.push(thread::spawn(move || lutex.with(|x| {
                assert!(!*x);
                *x = true;
                *x = false;
            })));
        }

        for i in v {
            i.join().unwrap();
        }
    }
}
