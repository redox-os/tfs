pub struct Stack<T> {
    head: AtomicPtr<Node<T>>,
}

struct Node<T> {
    data: T,
    next: *mut Node<T>,
}

impl<T> Stack<T> {
    const fn new() -> Stack<T> {
        Stack {
            head: AtomicPtr::new(0 as *const T),
        }
    }

    fn push(&self, t: T) {
        // Allocate the node, and immediately turn it into a `*mut` pointer.
        let n = Box::into_raw(Box::new(Node {
            data: t,
            next: null_mut(),
        }));

        loop {
            // Snapshot current head.
            let head = self.head.load(atomic::Ordering::Relaxed);

            // Update `next` pointer with snapshot.
            unsafe { (*n).next = head; }

            // If snapshot is still good, link in new node.
            if self.head.compare_and_swap(head, n, atomic::Ordering::Release) == head {
                break
            }
        }
    }

    fn take(&self) -> Stack<T> {
        // Replace the old head with a null pointer.
        self.head.swap(AtomicPtr::default(), atomic::Ordering::Acquire)
    }

    fn for_each(self, f: F)
    where F: Fn(T) {
        let mut node = self.head;
        // We traverse every node until the pointer is null.
        while !node.is_null() {
            // Read the node into an owned box.
            let bx = unsafe { Box::from_raw(head) };
            // Apply the provided closure.
            f(bx.data);
            // Go to the next link.
            node = bx.next;
        }
    }

    fn collect(&self) -> HashSet<T> {
        let mut hs = HashSet::new();
        self.take().for_each(|x| hs.insert(x));

        hs
    }
}
