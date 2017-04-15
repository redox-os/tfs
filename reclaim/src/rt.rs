use std::cell::Cell;

static UNREACHABLE: Stack<Box<Drop>> = Stack::new();
static LINKS: Stack<Link> = Stack::new();
static ADDING_LINKS: AtomicUsize = AtomicUsize::new(0);
static TICKS_BEFORE_GC: u16 = 2000;

thread_local!(static CLOCK: Cell<u16> = Cell::new(0));
thread_local!(static ACTIVE_READERS: Cell<usize> = Cell::new(0));

struct Link<T> {
    active: *const AtomicBool,
    pub ptr: *const T,
}

impl Link {
    fn new(ptr: *const T) {
        Link {
            ptr: ptr,
            // We allocate the atomic boolean on the heap as it is shared between the returned RAII
            // guard and the reader stack.
            released: Box::into_raw(Box::new(AtomicBool::new(false))),
        }
    }

    fn set_inactive(&self) {
        self.active.store(true);
    }

    unsafe fn destroy(self) {
        // Drop the atomic boolean stored on the heap.
        mem::drop_in_place(self.active);
    }
}

/// Set an item as unreachable.
///
/// "Unreachable" means that no new links to the item can be added. In other words, the only active
/// links are the one that were obtained prior to the item being marked unreachable.
pub fn set_unreachable<T>(item: Box<T>) {
    UNREACHABLE.push(item);
    tick();
}

pub fn gc() {
    // Check if there are any active readers.
    if ACTIVE_READERS.get() != 0 {
        // There are. We cannot garbage collect with active readers, hence we must skip it.
        return;
    }

    // Set the clock back to zero to delay the next garbage collection.
    CLOCK.set(0);

    // Spin until no other thread is adding links, ensuring the stack of active links is not
    // incomplete, and therefore collectable.
    while ADDING_LINKS.compare_and_swap(0, 1) != 0 {}

    // Initially, all the unreachable items are marked unused.
    let mut unused = UNREACHABLE.collect();
    let links = LINKS.take();

    // Set back the counter as our collection of the stacks is over.
    ADDING_LINKS.store(0);

    // Traverse the active links and update the set of unused items.
    links.for_each(|reader| {
        if reader.active.load() {
            // The reader is not released yet, and is thus considered active.

            // Remove the reader from the unused set and insert it back into the log (if it exists
            // in the unused set), as the link is active.
            unused.remove(reader.ptr).map(|x| UNREACHABLE.insert(x));
            // Put the reader back in the structure.
            LINKS.insert(reader);
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

pub fn linking<T, F>(f: F) -> T
where F: Fn(Linking) -> T {
    let mut active = ACTIVE_READERS.get();
    ACTIVE_READERS.set(active + 1);
    if active == 0 {
        ADDING_LINKS.fetch_add(1);
    }

    let linking = Linking;
    let ret = f(linking);

    active = ACTIVE_READERS.get();
    if active == 1 {
        ADDING_LINKS.fetch_sub(1);
    }
    ACTIVE_READERS.set(active - 1);

    ret
}

struct Linking;

impl Linking {
    pub fn link(&self, item: Link) {
        LINKS.push(item);
    }
}
