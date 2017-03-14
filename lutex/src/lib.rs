#![feature(conservative_impl_trait)]

extern crate crossbeam;
extern crate futures;

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};

use crossbeam::sync::{AtomicOption, SegQueue};
use futures::Future;

const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;

type Ret = ();

struct Worker<T, R, E> {
    ret: Arc<AtomicOption<Result<R, E>>>, // REEEE
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
    queue: SegQueue<Box<FnOnce(&mut T)>>,
}

impl<T> LutexInternal<T> {
    fn progress(&self) -> bool {
        if !self.locked.swap(true, ORDERING) {
            self.queue.pop()(unsafe { self.data.get() });
            self.locked.store(false, ORDERING);

            true
        } else { false }
    }
}

struct Lutex<T> {
    inner: Arc<LutexInternal<T>>,
}

impl<T> Lutex<T> {
    pub fn queue<R, E, F>(&self, f: F) -> impl Future<Item = R, Error = E>
        where F: FnOnce(&mut T) -> Result<R, E> {
        let ret = Arc::new(AtomicOption::new());

        let ret2 = ret.clone();
        self.inner.queue.push(Box::new(move |x| {
            ret2.swap(f(x), ORDERING);
        }));

        Worker {
            ret: ret,
            lutex: self.inner.clone(),
        }
    }
}
