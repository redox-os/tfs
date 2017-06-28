//! Treiber stacks.

use std::sync::atomic;
use std::ptr;
use {Atomic, Guard};

/// A Treiber stack.
///
/// Treiber stacks are one way to implement concurrent LIFO stack.
///
/// Treiber stacks builds on linked lists. They are lock-free and non-blocking. It can be compared
/// to transactional memory in that it repeats operations, if another thread changes it while.
///
/// The ABA problem is of course addressed through the API of this crate.
pub struct Treiber<T> {
    /// The head node.
    head: Atomic<Node<T>>,
}

impl<T> Drop for Treiber<T> {
    fn drop(&mut self) {
        // Due to the nature of Treiber stacks, there are no active guards of things within the
        // structure. They're all gone, thus we can safely mess with the inner structure.

        unsafe {
            // Call destructors on the stack.
            (**self.head.get_inner_mut().get_mut()).destroy();

            // To avoid an atomic load etc., we shortcut by calling the destructor us self. We can do
            // this without overhead, as we positively know that no guard into the structure exists.
            self.head.destroy_no_guards();
        }
    }
}

impl<T> Treiber<T> {
    /// Create a new, empty Treiber stack.
    pub fn new() -> Treiber<T> {
        Treiber {
            head: Atomic::default(),
        }
    }

    /// Pop an item from the stack.
    // TODO: Change this return type.
    pub fn pop(&self) -> Option<Guard<T>> {
        // TODO: Use `catch {}` here when it lands.
        // Read the head snapshot.
        let mut snapshot = self.head.load(atomic::Ordering::Acquire);

        // Unless the head snapshot is `None`, try to replace it with the tail.
        while let Some(node) = snapshot {
            // Attempt to replace the head with the tail of the head.
            match unsafe {
                self.head.compare_and_swap_raw(
                    node.as_raw(),
                    node.next as *mut Node<T>,
                    atomic::Ordering::Release,
                )
            } {
                // It succeeded; return the item.
                Ok(_) => return Some(node.map(|x| &x.item)),
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
        let mut snapshot = self.head.load(atomic::Ordering::Relaxed);
        let mut snapshot_ptr: Option<*const Node<T>>;

        // TODO: Use `catch {}` here when it lands.
        // Construct a node, which will be the new head.
        let mut node = Box::new(Node {
            item: item,
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

/// A node in the stack.
struct Node<T> {
    /// The data this node holds.
    item: T,
    /// The next node.
    next: *const Node<T>,
}

impl<T> Node<T> {
    /// Destroy the node and its precessors.
    ///
    /// # Safety
    ///
    /// As this can be called multiple times, it is marked unsafe.
    unsafe fn destroy(&mut self) {
        // FIXME: Since this is recursive (and although it is likely optimized out), there might be
        //        cases where this leads to stack overflow, given correct compilation flags and
        //        sufficiently many elements.

        // Recursively drop the next node, if it exists.
        if !self.next.is_null() {
            drop(Box::from_raw(self.next as *mut Node<T>));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    #[derive(Clone)]
    struct Dropper {
        d: Arc<AtomicUsize>,
    }

    impl Drop for Dropper {
        fn drop(&mut self) {
            self.d.fetch_add(1, atomic::Ordering::Relaxed);
        }
    }

    #[test]
    fn simple1() {
        let stack = Treiber::new();

        stack.push(1);
        stack.push(200);
        stack.push(44);

        assert_eq!(*stack.pop().unwrap(), 44);
        assert_eq!(*stack.pop().unwrap(), 200);
        assert_eq!(*stack.pop().unwrap(), 1);
        assert!(stack.pop().is_none());

        ::gc();
    }

    #[test]
    fn simple2() {
        let stack = Treiber::new();

        for _ in 0..16 {
            stack.push(1);
            stack.push(200);
            stack.push(44);

            assert_eq!(*stack.pop().unwrap(), 44);
            assert_eq!(*stack.pop().unwrap(), 200);
            stack.push(20000);

            assert_eq!(*stack.pop().unwrap(), 20000);
            assert_eq!(*stack.pop().unwrap(), 1);

            assert!(stack.pop().is_none());
            assert!(stack.pop().is_none());
            assert!(stack.pop().is_none());
            assert!(stack.pop().is_none());
        }

        ::gc();
    }

    #[test]
    fn simple3() {
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

    #[test]
    fn increment() {
        let stack = Arc::new(Treiber::<u64>::new());
        stack.push(0);
        let mut j = Vec::new();

        // 16 times, we add the numbers from 0 to 1000 to the only element in the stack.
        for _ in 0..16 {
            let s = stack.clone();
            j.push(thread::spawn(move || {
                for n in 0..1001 {
                    loop {
                        if let Some(x) = s.pop() {
                            s.push(*x + n);
                            break;
                        }
                    }
                }
            }));
        }

        for i in j {
            i.join().unwrap();
        }

        assert_eq!(*stack.pop().unwrap(), 16 * 1000 * 1001 / 2);
    }

    #[test]
    fn sum() {
        let stack = Arc::new(Treiber::<i64>::new());
        let mut j = Vec::new();

        for _ in 0..1000 {
            stack.push(10);
        }

        // We preserve the sum of the stack's elements.
        for _ in 0..16 {
            let s = stack.clone();
            j.push(thread::spawn(move || {
                for _ in 0..100000 {
                    loop {
                        if let Some(a) = s.pop() {
                            loop {
                                if let Some(b) = s.pop() {
                                    s.push(*a + 1);
                                    s.push(*b - 1);

                                    break;
                                }
                            }

                            break;
                        }
                    }
                }
            }));
        }

        for i in j {
            i.join().unwrap();
        }

        let mut sum = 0;
        while let Some(x) = stack.pop() {
            sum += *x;
        }
        assert_eq!(sum, 10000);
    }

    #[test]
    fn drop() {
        let drops = Arc::new(AtomicUsize::default());
        let stack = Arc::new(Treiber::new());

        let d = Dropper {
            d: drops.clone(),
        };

        let mut j = Vec::new();
        for _ in 0..16 {
            let d = d.clone();
            let stack = stack.clone();

            j.push(thread::spawn(move || {
                for _ in 0..20 {
                    stack.push(d.clone());
                }

                stack.pop();
                stack.pop();
            }))
        }

        for i in j {
            i.join().unwrap();
        }

        ::gc();
        // The 16 are for the `d` variable in the loop above.
        assert_eq!(drops.load(atomic::Ordering::Relaxed), 32 + 16);

        // Drop the last arc.
        let _ = stack;
        ::gc();

        assert_eq!(drops.load(atomic::Ordering::Relaxed), 200 + 16);
    }
}
