use std::sync::atomic::{self, AtomicPtr};

pub struct Stack<T> {
    head: AtomicPtr<Node<T>>,
}

struct Node<T> {
    data: T,
    next: *mut Node<T>,
}

impl<T> Stack<T> {
    fn new() -> Stack<T> {
        Stack {
            head: AtomicPtr::default,
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

    fn take_each(&self, f: F)
    where F: Fn(T) {
        // Replace the old head with a null pointer.
        let mut node = self.head.swap(AtomicPtr::default(), atomic::Ordering::Acquire);

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
}

struct RawReader {
    active: *const AtomicBool,
    ptr: *const T,
}

impl RawReader {
    unsafe fn destroy(self) {
        // Drop the atomic boolean stored on the heap.
        mem::drop_in_place(self.active);
    }
}

struct Reader<'a, T> {
    raw: RawReader,
    _marker: PhantomData<'a>,
}

impl<'a, T> Reader<'a, T> {
    fn drop(&mut self) {
        self.raw.active.store(true);
    }
}

struct Atomic<T> {
    inner: AtomicPtr<T>,
    snapshots: Stack<Box<T>>,
    readers: Stack<RawReader>,
    flags: AtomicUsize,
}

impl<T> Atomic<T> {
    fn new(inner: T) -> Atomic<T> {
        Atomic {
            inner: AtomicPtr::new(Box::into_raw(Box::new())),
            snapshots: Stack::new(),
            readers: Stack::new(),
            // Initially no one is accessing the readers stack nor garbage collecting, so the flags
            // are set to zero.
            flags: AtomicUsize::new(0),
        }
    }

    fn gc(&self) {
        // Spin until no thread is currently modifying the stacks. This prevents premature frees in
        // the thread which is currently pushing to `self.readers`.
        loop {
            // Read the flags, and if no readers or garbage collectors, activate garbage
            // collection.
            let flags = self.flags.compare_and_swap(0, 1, atomic::Ordering::Relaxed);
            if flags == 0 {
                // Currently, no one accesses the readers stack and the CAS above means that the
                // lowest bitflag have been set, indicating that a garbage collection is now
                // active.
                break;
            } else if flags & 1 == 1 {
                // Another thread is currently garbage collection. No need for this thread doing
                // the same.
                return;
            }
        }

        // We've now set the lowest bit in `self.active` namely the flag defining if we are
        // collecting.

        // Initially, every snapshot is marked unused.
        let mut unused = self.snapshots.collect();

        // Traverse the readers and update the reference counts.
        self.readers.take_each(|reader| {
            if reader.active.load() {
                // The reader is not released yet, and is thus considered active.

                // Remove the reader from the unused set and insert it back into the log (if it
                // exists in the unused set), as the snapshot is active.
                unused.remove(reader.ptr).map(|x| self.snapshots.insert(x));
                // Put the reader back in the structure.
                self.readers.insert(reader);
            } else {
                // The reader was released. Destroy it.
                reader.destroy();
            }
        });

        // Unset the bitflag defining if currently garbage collecting.
        self.flags.fetch_and(!1, atomic::Ordering::Relaxed);
    }

    fn get(&self) -> Reader {
        // Increment the number of threads currently pushing to the readers stack. We add two to
        // account for the LSB being a separate bitflag. This ensures that the read snapshot isn't
        // freed while registering it in the reader stack.
        self.flags.fetch_add(2, atomic::Ordering::Relaxed);

        // Construct the raw reader.
        let reader = RawReader {
            // Load a snapshot of the pointer.
            ptr: self.inner.load(atomic::Ordering::Relaxed),
            // We allocate the atomic boolean on the heap as it is shared between the returned RAII
            // guard and the reader stack.
            released: Box::into_raw(Box::new(AtomicBool::new(false))),
        };

        // Register the reader through the reader stack, ensuring that it is not freed before the
        // RAII guard drops (`reader.release` is set to `true`).
        self.readers.push(reader);

        // Revert the original increment.
        self.flags.fetch_sub(2, atomic::Ordering::Relaxed);

        Reader {
            raw: reader,
            _marker: PhantomData,
        }
    }

    fn set(&self, new: Box<T>) {
        // Replace the inner by the new value.
        let old = self.inner.swap(Box::into_raw(new), atomic::Ordering::Relaxed);
        // Push the old pointer to the snapshot stack.
        self.snapshots.push(Box::from_raw(old));
    }
}
