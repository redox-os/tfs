//! Software transactional memory.

use {Atomic, Guard};
use std::sync::atomic;

/// A software transactional memory container.
pub struct Stm<T> {
    /// The inner data.
    inner: Atomic<T>,
}

impl<T> Stm<T> {
    /// Create a new STM container.
    pub fn new(data: Option<Box<T>>) -> Stm<T> {
        Stm {
            inner: Atomic::new(data),
        }
    }

    /// Update the data.
    ///
    /// This applies closure `f` to the data of `self`. If the data isn't updated in the meantime,
    /// the change will applied. Otherwise, the closure is reevaluated.
    pub fn update<F>(&self, f: F)
    where
        F: Fn(Option<Guard<T>>) -> Option<Box<T>>,
        T: 'static,
    {
        loop {
            // Read a snapshot of the current data.
            let snapshot = self.inner.load(atomic::Ordering::Acquire);
            // Construct a pointer from this guard.
            let snapshot_ptr = snapshot.as_ref().map(Guard::as_ptr);
            // Evaluate the closure on the snapshot.
            let ret = f(snapshot);

            // If the snapshot pointer is still the same, update the data to the closure output.
            if self.inner.compare_and_store(snapshot_ptr, ret, atomic::Ordering::Release).is_ok() {
                break;
            }
        }
    }

    /// Read the container.
    pub fn load(&self) -> Option<Guard<T>> {
        self.inner.load(atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::sync::Arc;

    #[test]
    fn single_threaded() {
        let stm = Stm::new(None);

        stm.update(|_| Some(Box::new(4)));
        stm.update(|x| Some(Box::new(*x.unwrap() + 1)));
        stm.update(|x| {
            assert!(*x.unwrap() == 5);
            None
        });
        assert!(stm.load().is_none());
    }

    #[test]
    fn multi_threaded() {
        let stm = Arc::new(Stm::new(Some(Box::new(0))));

        let mut j = Vec::new();
        for _ in 0..16 {
            let stm = stm.clone();
            j.push(thread::spawn(move || {
                for _ in 0..1_000_000 {
                    stm.update(|x| Some(Box::new(*x.unwrap() + 1)))
                }
            }))
        }

        for i in j {
            i.join().unwrap();
        }

        assert_eq!(*stm.load().unwrap(), 16_000_000);
    }
}
