use std::sync::atomic;
use std::{mem, ptr};
use {Atomic, Guard};

pub struct Treiber<T> {
    head: Atomic<Node<T>>,
}

struct Node<T> {
    data: T,
    next: *const Node<T>,
}

impl<T> Treiber<T> {
    pub fn new() -> Treiber<T> {
        Treiber {
            head: Atomic::default(),
        }
    }

    pub fn pop(&self, item: T) -> Option<Guard<T>> {
        // TODO: Use `catch {}` here when it lands.
        let mut head = self.head.load(atomic::Ordering::Acquire);
        while let Some(node) = head {
            // TODO: This should be something that ignores the guard creation when the CAS
            //       fails, because it's expensive to do and not used anyway. It should be easy
            //       enough to implement, but I am struggling to come up with a good name for
            //       the method.
            match unsafe {
                self.head.compare_and_swap_raw(
                    &*node,
                    node.next as *mut Node<T>,
                    atomic::Ordering::Release
                )
            } {
                Ok(_) => return Some(node.map(|x| &x.data)),
                Err(new_head) => head = new_head,
            }
        }

        None
    }

    pub fn push(&self, item: T)
    where T: 'static {
        // Load the current head.
        let mut current = self.head.load(atomic::Ordering::Acquire);
        let mut current_ptr: Option<*const Node<T>>;

        // TODO: Use `catch {}` here when it lands.
        // Construct a node, which will be the new head.
        let mut node = Box::new(Node {
            data: item,
            // Placeholder; we will replace it with an actual value in the loop.
            next: ptr::null(),
        });

        loop {
            // Derive the nullable current pointer from the current head.
            current_ptr = current.as_ref().map(Guard::as_raw);
            // Construct the next-pointer of the new node from the current head.
            node.next = current_ptr.unwrap_or(ptr::null());

            // TODO: This should be something that ignores the guard creation when the CAS
            //       succeeds, because it's expensive to do and not used anyway. It should be easy
            //       enough to implement, but I am struggling to come up with a good name for the
            //       method.

            // CAS from the read pointer to the new head.
            match self.head.compare_and_swap(current_ptr, Some(node), atomic::Ordering::Release) {
                // If it succeeds, the item has been pushed.
                Ok(_) => break,
                // If it fails, we will retry the CAS with updated values.
                Err((new_head, Some(node2))) => {
                    // Update the current head.
                    current = new_head;
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
