use std::sync::atomic::{self, AtomicPtr};
use std::cell::Cell;

static GARBAGE: Stack<Box<Drop>> = Stack::new();
static ACTIVE_SNAPSHOTS: Stack<RawSnapshot> = Stack::new();
// TODO: rename, name's shite
static PUSHING_TO_ACTIVE_SNAPSHOTS: AtomicUsize = AtomicUsize::new(0);
static TICKS_BEFORE_GC: u16 = 2000;

thread_local!(static CLOCK: Cell<u16> = Cell::new(0));
thread_local!(static ACTIVE_READERS: Cell<usize> = Cell::new(0));

pub fn gc() {
    // Check if there are any active readers.
    if ACTIVE_READERS.get() != 0 {
        // There are. We cannot garbage collect with active readers, hence we must skip it.
        return;
    }

    // Set the clock back to zero to delay the next garbage collection.
    CLOCK.set(0);

    // Spin until no other thread is pushing snapshots, ensuring the stack of active snapshot is
    // not incomplete, and therefore collectable.
    while PUSHING_TO_ACTIVE_SNAPSHOTS.compare_and_swap(0, 1) != 0 {}

    // Initially, every garbage is marked unused.
    let mut unused = GARBAGE.collect();
    let snapshots = ACTIVE_SNAPSHOTS.take();

    // Set back the counter as our collection of the stacks is over.
    PUSHING_TO_ACTIVE_SNAPSHOTS.store(0);

    // Traverse the active snapshots and update the reference counts.
    snapshots.for_each(|reader| {
        if reader.active.load() {
            // The reader is not released yet, and is thus considered active.

            // Remove the reader from the unused set and insert it back into the log (if it
            // exists in the unused set), as the garbage is active.
            unused.remove(reader.ptr).map(|x| GARBAGE.insert(x));
            // Put the reader back in the structure.
            ACTIVE_SNAPSHOTS.insert(reader);
        } else {
            // The reader was released. Destroy it.
            reader.destroy();
        }
    });
}

pub fn tick() {
    let clock = CLOCK.get();
    if clock == TICKS_BEFORE_GC {
        gc();
    } else {
        CLOCK.set(clock + 1);
    }
}

pub fn read<T, F>(f: F) -> T
where F: Fn(Reader) -> T {
    let mut active = ACTIVE_READERS.get();
    ACTIVE_READERS.set(active + 1);
    if active == 0 {
        PUSHING_TO_ACTIVE_SNAPSHOTS.fetch_add(1);
    }

    let reader = Reader;
    let ret = f(&reader);

    active = ACTIVE_READERS.get();
    if active == 1 {
        PUSHING_TO_ACTIVE_SNAPSHOTS.fetch_sub(1);
    }
    ACTIVE_READERS.set(active - 1);

    ret
}

pub struct Reader;

impl Reader {
    pub fn load<T>(&self, a: &Atomic<T>) -> Snapshot<T> {
        // Construct the raw reader.
        let reader = RawSnapshot {
            // Load a snapshot of the pointer.
            ptr: self.inner.load(atomic::Ordering::Relaxed),
            // We allocate the atomic boolean on the heap as it is shared between the returned RAII
            // guard and the reader stack.
            released: Box::into_raw(Box::new(AtomicBool::new(false))),
        };

        // Register the reader through the reader stack, ensuring that it is not freed before the
        // RAII guard drops (`reader.release` is set to `true`).
        ACTIVE_SNAPSHOTS.push(reader);

        Snapshot {
            raw: reader,
            _marker: PhantomData,
        }
    }
}

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
        self.take_each(|x| hs.insert(x));

        hs
    }
}

struct RawSnapshot<T> {
    active: *const AtomicBool,
    ptr: *const T,
}

impl RawSnapshot {
    unsafe fn destroy(self) {
        // Drop the atomic boolean stored on the heap.
        mem::drop_in_place(self.active);
    }
}

struct Snapshot<'a, T> {
    raw: RawSnapshot,
    _marker: PhantomData<'a>,
}

impl<'a, T> Snapshot<'a, T> {
    fn drop(&mut self) {
        self.raw.active.store(true);
    }
}

pub struct Atomic<T> {
    inner: AtomicPtr<T>,
}

impl<T> Atomic<T> {
    pub fn new(inner: T) -> Atomic<T> {
        Atomic {
            inner: AtomicPtr::new(Box::into_raw(Box::new(inner))),
        }
    }

    pub fn load(&self) -> Snapshot<T> {
        read(|r| r.load(self))
    }

    pub fn store(&self, new: Box<T>) {
        // Replace the inner by the new value.
        let old = self.inner.swap(Box::into_raw(new), atomic::Ordering::Relaxed);
        // Push the old pointer to the garbage stack.
        GARBAGE.push(Box::from_raw(old));

        tick();
    }
}
