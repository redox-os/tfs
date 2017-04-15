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
