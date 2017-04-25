pub enum State {
    Free,
    Dead,
    Protect(*const u8),
}

pub struct Hazard {
    ptr: AtomicUsize,
}

impl Hazard {
    pub fn blocked() -> Hazard {
        Hazard {
            ptr: AtomicUsize::new(0),
        }
    }

    pub fn block(&self) {
        self.ptr.store(0, atomic::Ordering::Release);
    }

    pub fn set(&self, new: State) {
        self.ptr.store(match new {
            State::Free => 1,
            State::Dead => 2,
            State::Protect(ptr) => ptr as usize,
        }, atomic::Ordering::Release);
    }

    pub fn get(&self) -> State {
        loop {
            return match self.ptr.load(atomic::Ordering::Acquire) {
                // 0 means that the hazard is blocked by another thread, and we must loop until it
                // assumes another state.
                0 => continue,
                1 => State::Free,
                2 => State::Dead,
                ptr => State::Protect(ptr as *const u8)
            };
        }
    }
}

/// Create a new hazard reader-writer pair.
///
/// This creates a new hazard pair in blocked state.
pub fn create() -> (Writer, Reader) {
    let ptr = Box:into_raw(Box::new(Hazard::blocked()));

    (Writer {
        ptr: ptr,
    }, Reader {
        ptr: ptr,
    })
}

pub struct Reader {
    ptr: *mut Hazard,
}

impl Reader {
    pub fn get(&self) -> State {
        self.ptr.get()
    }

    pub unsafe fn destroy(self) {
        debug_assert!(self.get() == State::Dead, "Prematurely freeing an active hazard.");
        Box::from_raw(self.ptr);
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        panic!("Hazard readers ought to be destroyed manually through the `destroy` method.");
    }
}

pub struct Writer {
    ptr: *mut Hazard,
}

impl ops::Deref for Writer {
    type Target = Hazard;

    fn deref(&self) -> &Hazard {
        unsafe {
            &*self.ptr
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.set(State::Dead);
    }
}
