//! Multi-producer single-consumer queues.
//!
//! Since the standard library's implementation of `mpsc` requires us to clone the senders in
//! advance, such that we cannot store them in our global state outside a lock, we must implement
//! our own `mpsc` queue.
//!
//! Right now, the implementation is really nothing but a wrapper around `Mutex<Vec<T>>`, and
//! although this is reasonably fast as the lock is only held for very short time, it is
//! sub-optimal, and blocking.

use parking_lot::Mutex;
use std::sync::Arc;
use std::mem;

/// Create a MPSC pair.
///
/// This creates a "channel", i.e. a pair of sender and receiver connected to each other.
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    // Create a new ARC.
    let end = Arc::new(Mutex::new(Vec::new()));

    (Sender {
        inner: end.clone(),
    }, Receiver {
        inner: end,
    })
}

/// The sender of a MPSC channel.
pub struct Sender<T> {
    /// The wrapped end.
    inner: Arc<Mutex<Vec<T>>>,
}

impl<T> Sender<T> {
    /// Send an item to this channel.
    pub fn send(&self, item: T) {
        // Lock the vector, and push.
        self.inner.lock().push(item);
    }
}

/// The receiver of a MPSC channel.
pub struct Receiver<T> {
    /// The wrapped end.
    inner: Arc<Mutex<Vec<T>>>,
}

impl<T> Receiver<T> {
    /// Receive all the elements in the queue.
    ///
    /// This takes all the elements and applies the given closure to them in an unspecified order.
    pub fn recv_all(&self) -> Vec<T> {
        // Lock the vector, and replace it by an empty vector, then iterate.
        mem::replace(&mut *self.inner.lock(), Vec::new())
    }
}
