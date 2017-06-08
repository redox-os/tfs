//! Treiber stacks.

use std::sync::atomic;
use std::{mem, ptr};
use {Atomic, Guard};

/// A Treiber stack.
pub struct Treiber<T> {
    head: Atomic<Node<T>>,
}

struct Node<T> {
    data: T,
    next: *const Node<T>,
}

impl<T> Treiber<T> {
    /// Create a new, empty Treiber stack.
    pub fn new() -> Treiber<T> {
        Treiber {
            head: Atomic::default(),
        }
    }

    /// Pop an item from the stack.
    pub fn pop(&self) -> Option<Guard<T>> {
        // TODO: Use `catch {}` here when it lands.
        // Read the head snapshot.
        let mut snapshot = self.head.load(atomic::Ordering::Acquire);

        // Unless the head snapshot is `None`, try to replace it with the tail.
        while let Some(node) = snapshot {
            // Attempt to replace the head with the tail of the head.
            match unsafe {
                self.head.compare_and_swap_raw(
                    &*node,
                    node.next as *mut Node<T>,
                    atomic::Ordering::Release,
                )
            } {
                // It succeeded; return the item.
                Ok(_) => return Some(node.map(|x| &x.data)),
                // It failed, update the head snapshot and continue.
                Err(new) => snapshot = new,
            }
        }

        // As the head was empty, there is nothing to pop.
        None
    }

    /// Push an item to the stack.
    pub fn push(&self, item: T)
    where T: 'static {
        // Load the head snapshot.
        let mut snapshot = self.head.load(atomic::Ordering::Acquire);
        let mut snapshot_ptr: Option<*const Node<T>>;

        // TODO: Use `catch {}` here when it lands.
        // Construct a node, which will be the new head.
        let mut node = Box::new(Node {
            data: item,
            // Placeholder; we will replace it with an actual value in the loop.
            next: ptr::null(),
        });

        loop {
            // Derive the nullable snapshot pointer from the head snapshot.
            snapshot_ptr = snapshot.as_ref().map(Guard::as_raw);
            // Construct the next-pointer of the new node from the head snapshot.
            node.next = snapshot_ptr.unwrap_or(ptr::null());

            // TODO: This should be something that ignores the guard creation when the CAS
            //       succeeds, because it's expensive to do and not used anyway. It should be easy
            //       enough to implement, but I am struggling to come up with a good name for the
            //       method.

            // CAS from the read pointer to the new head.
            match self.head.compare_and_swap(snapshot_ptr, Some(node), atomic::Ordering::Release) {
                // If it succeeds, the item has been pushed.
                Ok(_) => break,
                // If it fails, we will retry the CAS with updated values.
                Err((new_head, Some(node2))) => {
                    // Update the head snapshot.
                    snapshot = new_head;
                    // Put the box we gave back to the variable where it belongs.
                    node = node2;
                },
                // This should never be reached as we gave an argument which was unconditionally
                // `Some`.
                _ => unreachable!(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::sync::Arc;

    #[test]
    fn single_thread() {
        let stack = Treiber::new();

        for i in 0..10000 {
            stack.push(i);
        }

        for i in (0..10000).rev() {
            assert_eq!(*stack.pop().unwrap(), i);
        }

        for i in 0..10000 {
            stack.push(i);
        }

        for i in (0..10000).rev() {
            assert_eq!(*stack.pop().unwrap(), i);
        }

        assert!(stack.pop().is_none());
        assert!(stack.pop().is_none());
        assert!(stack.pop().is_none());
        assert!(stack.pop().is_none());
    }

    #[test]
    fn push_pop() {
        let stack = Arc::new(Treiber::new());
        let mut j = Vec::new();
        for _ in 0..16 {
            let s = stack.clone();
            j.push(thread::spawn(move || {
                for _ in 0..10_000_000 {
                    s.push(23);
                    assert_eq!(*s.pop().unwrap(), 23);
                }
            }));
        }

        for i in j {
            i.join().unwrap();
        }
    }
}
