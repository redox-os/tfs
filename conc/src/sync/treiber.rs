//! Treiber stacks.

use std::sync::atomic::{self, AtomicPtr};
use std::marker::PhantomData;
use std::ptr;
use {Guard, add_garbage_box};

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
    head: AtomicPtr<Node<T>>,
    /// Make the `Sync` and `Send` (and other OIBITs) transitive.
    _marker: PhantomData<T>,
}

impl<T> Treiber<T> {
    /// Create a new, empty Treiber stack.
    pub fn new() -> Treiber<T> {
        Treiber {
            head: AtomicPtr::default(),
            _marker: PhantomData,
        }
    }

    /// Pop an item from the stack.
    // TODO: Change this return type.
    pub fn pop(&self) -> Option<Guard<T>> {
        // TODO: Use `catch {}` here when it lands.
        // Read the head snapshot.
        let mut snapshot = Guard::maybe_new(|| unsafe {
            self.head.load(atomic::Ordering::Acquire).as_ref()
        });

        // Unless the head snapshot is `None`, try to replace it with the tail.
        while let Some(old) = snapshot {
            // Attempt to replace the head with the tail of the head.
            snapshot = Guard::maybe_new(|| unsafe {
                self.head.compare_and_swap(
                    old.as_ptr() as *mut _,
                    old.next as *mut Node<T>,
                    atomic::Ordering::Release,
                ).as_ref()
            });

            // If it match, we are done as the previous head node was replaced by the tail, popping
            // the top element. The element we return is the one carried by the previous head.
            if let Some(ref new) = snapshot {
                if new.as_ptr() == old.as_ptr() {
                    // As we overwrote the old head (the CAS was successful), we must queue its
                    // deletion.
                    unsafe { add_garbage_box(old.as_ptr()); }
                    // Map the guard to refer the item.
                    return Some(old.map(|x| &x.item));
                }
            } else {
                // Short-circuit.
                break;
            }
        }

        // As the head was empty, there is nothing to pop.
        None
    }

    /// Push an item to the stack.
    pub fn push(&self, item: T)
    where T: 'static {
        // Load the head snapshot.
        let mut snapshot = Guard::maybe_new(|| unsafe {
            self.head.load(atomic::Ordering::Relaxed).as_ref()
        });

        // TODO: Use `catch {}` here when it lands.
        // Construct a node, which will be the new head.
        let mut node = Box::into_raw(Box::new(Node {
            item: item,
            // Placeholder; we will replace it with an actual value in the loop.
            next: ptr::null_mut(),
        }));

        loop {
            // Construct the next-pointer of the new node from the head snapshot.
            let next = snapshot.map_or(ptr::null_mut(), |x| x.as_ptr() as *mut _);
            unsafe { (*node).next = next; }

            // CAS from the read pointer (that is, the one we placed as `node.next`) to the new
            // head.
            match Guard::maybe_new(|| unsafe {
                // TODO: This should be something that ignores the guard creation when the CAS
                // succeeds, because it's expensive to do and not used anyway. It should be easy
                // enough to implement, but I am struggling to come up with a good name for the
                // method.
                self.head.compare_and_swap(next, node, atomic::Ordering::Release).as_ref()
            }) {
                // If it succeeds (that is, the pointers matched and the CAS ran), the item has
                // been pushed.
                Some(ref new) if new.as_ptr() == next => break,
                None if next.is_null() => break,
                // If it fails, we will retry the CAS with updated values.
                new => snapshot = new,
            }
        }
    }
}

impl<T> Drop for Treiber<T> {
    fn drop(&mut self) {
        // Due to the nature of Treiber stacks, there are no active guards of things within the
        // structure. They're all gone, thus we can safely mess with the inner structure.

        unsafe {
            let ptr = *self.head.get_mut();

            if !ptr.is_null() {
                // Call destructors on the stack.
                (*ptr).destroy();
                // Finally deallocate the pointer itself.
                // TODO: Figure out if it is sound if this destructor panics.
                drop(Box::from_raw(ptr));
            }
        }
    }
}

/// A node in the stack.
struct Node<T> {
    /// The data this node holds.
    item: T,
    /// The next node.
    next: *mut Node<T>,
}

impl<T> Node<T> {
    /// Destroy the node and its precessors.
    ///
    /// This doesn't call the destructor on `T`.
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
            // Recurse to the next node.
            (*self.next).destroy();
            // Now that all of the children of the next node has been dropped, drop the node
            // itself.
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
    fn empty() {
        for _ in 0..1000 {
            let b = Box::new(20);
            Treiber::<u8>::new();
            assert_eq!(*b, 20);
        }
    }

    #[test]
    fn just_push() {
        let stack = Treiber::new();
        stack.push(1);
        stack.push(2);
        stack.push(3);
        drop(stack);
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
                for _ in 0..1_000_000 {
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
    fn drop1() {
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
        drop(stack);
        ::gc();

        assert_eq!(drops.load(atomic::Ordering::Relaxed), 20 * 16 + 16);
    }

    #[test]
    #[should_panic]
    fn panic_in_dtor() {
        struct A;
        impl Drop for A {
            fn drop(&mut self) {
                panic!();
            }
        }

        let stack = Treiber::new();
        stack.push(Box::new(A));
        stack.push(Box::new(A));
        stack.push(Box::new(A));
    }
}
