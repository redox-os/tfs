use std::sync::atomic::{self, AtomicPtr};

use std::ptr::{self, null_mut};
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering::{Relaxed, Release, Acquire};

pub struct Stack<T> {
    head: AtomicPtr<Node<T>>,
}

struct Node<T> {
    data: T,
    next: *mut Node<T>,
}

impl<T> Stack<T> {
    pub fn new() -> Stack<T> {
        Stack {
            head: AtomicPtr::new(null_mut()),
        }
    }

    pub fn push(&self, t: T) {
        // Allocate the node, and immediately turn it into a `*mut` pointer.
        let n = Box::into_raw(Box::new(Node {
            data: t,
            next: null_mut(),
        }));

        loop {
            // Snapshot current head.
            let head = self.head.load(Relaxed);

            // Update `next` pointer with snapshot.
            unsafe { (*n).next = head; }

            // If snapshot is still good, link in new node.
            if self.head.compare_and_swap(head, n, Release) == head {
                break
            }
        }
    }
}

struct RawReader {
    active: *const AtomicBool,
    ptr: *const T,
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
    collecting: AtomicBool,
    inner: AtomicPtr<T>,
    readers: Stack<RawReader>,
    snapshots: Stack<Box<T>>,
}

impl<T> Atomic<T> {
    fn gc(&self) {
        if !self.collecting.swap(true, Ordering::Relaxed) {
            // Initially, every snapshot is marked unused.
            let mut unused = self.snapshots.collect();

            // Traverse the readers and update the reference counts.
            self.readers.take_each(|reader| {
                if reader.active.load() {
                    // The reader is not released yet, and is thus considered active.

                    // Remove the reader from the unused set and insert it back into the log, as
                    // the snapshot is active.
                    self.snapshots.insert(unused.remove(reader.ptr).unwrap());
                    // Put the reader back in the structure.
                    self.readers.insert(reader);
                }
            });

            self.collecting.store(true);
        }
    }
}
