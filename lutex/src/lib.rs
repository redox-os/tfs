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

pub struct Worker<T, R, E> {
    ret: Arc<AtomicOption<Result<R, E>>>,
    lutex: Arc<LutexInternal<T>>,
}

impl<T, R, E> Future for Worker<T, R, E> {
    type Item = R;
    type Error = E;

    fn poll(&mut self) -> futures::Poll<R, E> {
        if self.lutex.progress() {
            match self.ret.take(ORDERING) {
                Some(Ok(x)) => Ok(futures::Async::Ready(x)),
                Some(Err(x)) => Err(x),
                None => Ok(futures::Async::NotReady),
            }
        } else {
            Ok(futures::Async::NotReady)
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

    fn progress(&self) -> bool {
        if !self.locked.swap(true, ORDERING) {
            let f = self.queue.try_pop().unwrap();
            f(self.data.get());
            self.locked.store(false, ORDERING);
            self.parked.try_pop().map(|task| task.unpark());

            true
        } else {
            self.parked.push(task::park());
            false
        }
    }
}

pub struct Lutex<T> {
    inner: Arc<LutexInternal<T>>,
}

impl<T> Lutex<T> {
    pub fn new(data: T) -> Lutex<T> {
        Lutex {
            inner: Arc::new(LutexInternal::new(data)),
        }
    }

    pub fn with<F: 'static>(&self, f: F) -> impl Future<Item = (), Error = ()>
        where F: FnOnce(&mut T) {
        self.try_with::<(), (), _>(|x| {
            f(x);
            Ok(())
        })
    }

    pub fn try_with<R: 'static, E: 'static, F: 'static>(&self, f: F) -> impl Future<Item = R, Error = E>
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
