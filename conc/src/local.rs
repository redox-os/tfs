//! The thread-local state.

use std::{mem, thread};
use std::cell::RefCell;
use {global, hazard, guard, debug, settings};
use garbage::Garbage;

thread_local! {
    /// The state of this thread.
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Add new garbage to be deleted.
///
/// This garbage is pushed to a thread-local queue. When enough garbage is accumulated in the
/// thread, it is exported to the global state.
pub fn add_garbage(garbage: Garbage) {
    // Print message in debug mode.
    debug::exec(|| println!("Adding garbage: {:?}", garbage));
    // Since this function can trigger a GC, it must not be called inside a guard constructor.
    guard::debug_assert_no_create();

    if STATE.state() == thread::LocalKeyState::Destroyed {
        // The state was deinitialized, so we must rely on the global state for queueing garbage.
        global::export_garbage(vec![garbage]);
    } else {
        // Add the garbage.
        if STATE.with(|s| s.borrow_mut().add_garbage(garbage)) {
            // The local state exported garbage to the global state, hence we must tick in order to
            // ensure that the garbage is periodically collected.
            global::tick();
        }
    }
}

/// Get a blocked hazard.
///
/// If possible, this will simply pop one of the thread-local cache of hazards. Otherwise, one must
/// be registered in the global state.
///
/// # Fence
///
/// This does not fence, and you must thus be careful with updating the value afterwards, as
/// reordering can happen, meaning that the hazard has not been blocked yet.
pub fn get_hazard() -> hazard::Writer {
    if STATE.state() == thread::LocalKeyState::Destroyed {
        // The state was deinitialized, so we must rely on the global state for creating new
        // hazards.
        global::create_hazard()
    } else {
        STATE.with(|s| s.borrow_mut().get_hazard())
    }
}

/// Free a hazard.
///
/// This frees a hazard to the thread-local cache of hazards.
///
/// It is important that the hazard is **not** in blocked state, as such thing can cause infinite
/// looping.
///
/// # Panics
///
/// This might panic in debug mode if the hazard given is in blocked state, as such thing can cause
/// infinite garbage collection cycle, or if the hazard is in dead state, as that means that it may
/// not be reusable (it could be destroyed).
pub fn free_hazard(hazard: hazard::Writer) {
    // Print message in debug mode.
    debug::exec(|| println!("Freeing hazard: {:?}", hazard));
    // Since this function can trigger a GC, it must not be called inside a guard constructor.
    guard::debug_assert_no_create();

    debug_assert!(!hazard.is_blocked(), "Illegally freeing a blocked hazards.");

    if STATE.state() == thread::LocalKeyState::Destroyed {
        // Since the state was deinitialized, we cannot store it for later reuse, so we are forced
        // to simply kill the hazard.
        hazard.kill();
    } else {
        STATE.with(|s| s.borrow_mut().free_hazard(hazard));
    }
}

/// Export the garbage of this thread to the global state.
///
/// This is useful for propagating accumulated garbage such that it can be destroyed by the next
/// garbage collection.
pub fn export_garbage() {
    // Since this function can trigger a GC, it must not be called inside a guard constructor.
    guard::debug_assert_no_create();

    // We can only export when the TLS variable isn't destroyed. Otherwise, there would be nothing
    // to export!
    if STATE.state() != thread::LocalKeyState::Destroyed {
        STATE.with(|s| s.borrow_mut().export_garbage());
        // We tick after the state is no longer reserved, as the tick could potentially call
        // destructor that access the TLS variable.
        global::tick();
    }
}

/// A thread-local state.
#[derive(Default)]
struct State {
    /// The cached garbage waiting to be exported to the global state.
    garbage: Vec<Garbage>,
    /// The cache of currently available hazards.
    ///
    /// We maintain this cache to avoid the performance hit of creating new hazards.
    ///
    /// The hazards in this vector are not necessarily in state "free". Only when a sufficient
    /// amount of available hazards has accumulated, they will be set to free. This means that we
    /// don't have to reset the state of a hazard after usage, giving a quite significant speed-up.
    available_hazards: Vec<hazard::Writer>,
    /// The hazards in the cache before this index are free.
    ///
    /// This number keeps track what hazards in `self.available_hazard` are set to state "free".
    /// Before this index, every hazard must be set to "free".
    ///
    /// It is useful for knowing when to free the hazards to allow garbage collection.
    available_hazards_free_before: usize,
}

impl State {
    /// Get the number of hazards in the cache which are not in state "free".
    fn non_free_hazards(&self) -> usize {
        self.available_hazards.len() - self.available_hazards_free_before
    }

    /// See `get_hazard()`.
    fn get_hazard(&mut self) -> hazard::Writer {
        // Check if there is hazards in the cache.
        if let Some(hazard) = self.available_hazards.pop() {
            // There is; we don't need to create a new hazard.

            // Since the hazard popped from the cache is not blocked, we must block the hazard to
            // satisfy the requirements of this function.
            hazard.block();
            hazard
        } else {
            // There is not; we must create a new hazard.
            global::create_hazard()
        }
    }

    /// See `free_hazard()`.
    fn free_hazard(&mut self, hazard: hazard::Writer) {
        // FIXME: This can lead to some subtle bugs, since the dtor is unpredictable as there is no
        //        way of predicting when the hazard is cleared.

        // Push the given hazard to the cache.
        self.available_hazards.push(hazard);

        // Check if we exceeded the limit.
        if self.non_free_hazards() > settings::get().max_non_free_hazards {
            // We did; we must now set the non-free hazards to "free".
            for i in &self.available_hazards[self.available_hazards_free_before..] {
                i.free();
            }

            // Update the counter such that we mark the new hazards set to "free".
            self.available_hazards_free_before = self.available_hazards.len();
        }
    }

    /// Queues garbage to destroy.
    ///
    /// Eventually the added garbage will be exported to the global state through
    /// `global::add_garbage()`.
    ///
    /// See `add_garbage` for more information.
    ///
    /// When this happens (i.e. the global state gets the garbage), it returns `true`. Otherwise,
    /// it returns `false`.
    fn add_garbage(&mut self, garbage: Garbage) -> bool {
        // Push the garbage to the cache of garbage.
        self.garbage.push(garbage);

        // Export the garbage if it exceeds the limit.
        // TODO: use memory instead of items as a metric.
        if self.garbage.len() > settings::get().max_garbage_before_export {
            self.export_garbage();
            true
        } else { false }
    }

    /// See `export_garbage()` for more information.
    fn export_garbage(&mut self) {
        // Print message in debug mode.
        debug::exec(|| println!("Exporting garbage."));

        // Clear the vector and export the garbage.
        global::export_garbage(mem::replace(&mut self.garbage, Vec::new()));
    }
}

impl Drop for State {
    fn drop(&mut self) {
        // Clear every hazard to "dead" state.
        for hazard in self.available_hazards.drain(..) {
            hazard.kill();
        }

        // The thread is exiting, thus we must export the garbage to the global state to avoid
        // memory leaks. It is very important that this does indeed not tick, as causing garbage
        // collection means accessing RNG state, a TLS variable, which cannot be done when, we are
        // here, after it has deinitialized.
        // TODO: Figure out a way we can tick anyway.
        self.export_garbage();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garbage::Garbage;
    use hazard;
    use std::thread;

    #[test]
    fn dtor_runs() {
        fn dtor(x: *const u8) {
            unsafe {
                *(x as *mut u8) = 1;
            }
        }

        for _ in 0..1000 {
            let b = Box::new(0);
            let h = get_hazard();
            h.protect(&*b);
            add_garbage(Garbage::new(&*b, dtor));
            ::gc();
            assert_eq!(*b, 0);
            ::gc();
            h.free();
            ::gc();
            assert_eq!(*b, 1);
        }
    }

    #[test]
    fn dtor_runs_cross_thread() {
        fn dtor(x: *const u8) {
            unsafe {
                *(x as *mut u8) = 1;
            }
        }

        for _ in 0..1000 {
            let b = Box::new(0);
            let bptr = &*b as *const _ as usize;
            let h = thread::spawn(move || {
                let h = get_hazard();
                h.protect(bptr as *const u8);
                h
            }).join().unwrap();
            add_garbage(Garbage::new(&*b, dtor));
            ::gc();
            assert_eq!(*b, 0);
            ::gc();
            h.free();
            ::gc();
            assert_eq!(*b, 1);
        }
    }

    #[test]
    fn clear_hazards() {
        let mut s = State::default();
        let mut v = Vec::new();
        for _ in 0..100 {
            let (w, r) = hazard::create();
            w.protect(0x1 as *const u8);
            v.push(r);
            s.free_hazard(w);
        }

        for i in &v[0..16] {
            assert_eq!(i.get(), hazard::State::Free);
        }

        mem::forget(v);
    }

    #[test]
    fn kill_hazards() {
        fn dtor(x: *const u8) {
            unsafe {
                *(x as *mut u8) = 1;
            }
        }

        for _ in 0..1000 {
            let b = thread::spawn(move || {
                let b = Box::new(0);
                let h = get_hazard();
                h.protect(&*b);
                add_garbage(Garbage::new(&*b, dtor));
                ::gc();
                assert_eq!(*b, 0);
                b
            }).join().unwrap();
            ::gc();
            assert_eq!(*b, 1);
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn debug_free_blocked() {
        use std::mem;

        let (writer, reader) = hazard::create();
        mem::forget(reader);

        free_hazard(writer);
    }
}
