use std::ptr;

fn swap_if_neq<T>(atomic: AtomicPtr<T>, cmp: *mut T, new: *mut T) -> *mut T {
    let prev = atomic.load(atomic::Ordering::Relaxed);
    loop {
        if prev == cmp {
            break;
        }

        let x = atomic.compare_and_swap(prev, new);
        if x == prev {
            break;
        } else {
            prev = x;
        }
    }
}

struct Channel<T> {
    head: AtomicPtr<Node<T>>,
}

struct Node<T> {
    item: T,
    next: AtomicPtr<Node<T>>,
}

impl<T> Channel<T> {
    fn push(&self, item: T) {
        loop {
            // Since we run into the ABA problem, if we read a snapshot through `load`, we must use
            // `swap` and temporarily remove the data, effectively blocking all other threads
            // messing with this channel.
            let head = self.head.swap(0x1 as *mut Node<T>, atomic::Ordering::Relaxed);

            // If the head is already blocked, try again.
            if head as usize == 1 {
                continue;
            }

            // Store the new node (this also unblocks the channel for other threads).
            self.head.store(Box::into_raw(Box::new(Node {
                item: item,
                next: head,
            })), atomic::Ordering::Relaxed);
        }
    }

    fn take_all<F>(&self, f: F)
    where F: Fn(&T) {
        let mut node;

        // Loop until the channel is not blocked.
        loop {
            node = swap_if_neq(1, ptr::null_mut());

            if node != 1 {
                break;
            }
        }

        while let Some(ptr) = node.get() {
            f(ptr.item);
            node = *ptr.next.get_mut();
        }
    }
}
