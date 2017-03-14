extern crate crossbeam;

const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;

type Ret = ();

struct Worker<T, R, E> {
    ret: Arc<AtomicOption<Result<T, R>>>,
    lutex: Arc<LutexInternal<T, R>>,
}

impl<T, R> Future for Worker<T, R> {
    type Item = T;
    type Error = E;

    fn poll(&mut self) -> futures::Poll<T, E> {
        self.lutex.queue.pop()();

        match self.ret.take() {
            Some(Ok(x)) => Ok(futures::Async::Ready(x)),
            Some(Err(x)) => Err(x),
            None => Ok(futures::Async::NotReady),
        }
    }
}

struct LutexInternal<T, R> {
    data: UnsafeCell<T>,
    locked: AtomicBool,
    queue: SegQueue<FnOnce(&mut T)>,
}

impl<T, R> Arc<LutexInternal<T, R>> {
    pub fn queue<R, E, F>(self, f: F) -> impl Future<R, E>
        where F: FnOnce(&mut T) -> Result<R, E> {
        let ret = Arc::new(AtomicOption::new());

        let ret2 = ret.clone();
        self.queue.push(Box::new(move |x| {
            ret2.swap(f(x), ORDERING);
        }))

        Worker {
            ret: ret,
            lutex: self,
        }
    }
}
