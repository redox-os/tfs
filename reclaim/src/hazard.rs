enum HazardState {
    Free,
    Dead,
    Protect(*const u8),
}

struct Hazard {
    ptr: AtomicUsize,
}

impl Hazard {
    fn blocked() -> Hazard {
        Hazard {
            ptr: AtomicUsize::new(0),
        }
    }

    fn block(&self) {
        self.ptr.store(0, atomic::Ordering::Release);
    }

    fn set(&self, new: HazardState) {
        self.ptr.store(match new {
            HazardState::Free => 1,
            HazardState::Dead => 2,
            HazardState::Protect(ptr) => ptr as usize,
        }, atomic::Ordering::Release);
    }

    fn get(&self) -> HazardState {
        loop {
            return match self.ptr.load(atomic::Ordering::Acquire) {
                // 0 means that the hazard is blocked by another thread, and we must loop until it
                // assumes another state.
                0 => continue,
                1 => HazardState::Free,
                2 => HazardState::Dead,
                ptr => HazardState::Protect(ptr as *const u8)
            };
        }
    }
}

/// Create a new hazard reader-writer pair.
///
/// This creates a new hazard pair in blocked state.
fn create() -> (HazardWriter, HazardReader) {
    let ptr = Box:into_raw(Box::new(Hazard::blocked()));

    (HazardWriter {
        ptr: ptr,
    }, HazardReader {
        ptr: ptr,
    })
}

struct HazardReader {
    ptr: *mut Hazard,
}

impl HazardReader {
    fn get(&self) -> HazardState {
        self.ptr.get()
    }

    unsafe fn destroy(self) {
        debug_assert!(self.get() == HazardState::Dead, "Prematurely freeing an active hazard.");
        Box::from_raw(self.ptr);
    }
}

impl Drop for HazardReader {
    fn drop(&mut self) {
        panic!("Hazard readers ought to be destroyed manually through the `destroy` method.");
    }
}

struct HazardWriter {
    ptr: *mut Hazard,
}

impl ops::Deref for HazardWriter {
    type Target = Hazard;

    fn deref(&self) -> &Hazard {
        unsafe {
            &*self.ptr
        }
    }
}

impl Drop for HazardWriter {
    fn drop(&mut self) {
        self.set(HazardState::Dead);
    }
}
