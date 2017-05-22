//! The thread-local state.

use std::mem;
use std::cell::RefCell;
use {global, hazard};
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
    STATE.with(|s| s.borrow_mut().add_garbage(garbage));
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
    STATE.with(|s| s.borrow_mut().get_hazard())
}

/// Free a hazard.
///
/// This frees a hazard to the thread-local cache of hazards.
pub fn free_hazard(hazard: hazard::Writer) {
    STATE.with(|s| s.borrow_mut().free_hazard(hazard));
}

/// Export the garbage of this thread to the global state.
///
/// This is useful for propagating accumulated garbage such that it can be destroyed by the next
/// garbage collection.
pub fn export_garbage() {
    STATE.with(|s| s.borrow_mut().export_garbage_and_tick());
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
        /// The maximal amount of hazards before cleaning up.
        ///
        /// With "cleaning up" we mean setting the state of the hazards to "free" in order to allow
        /// garbage collection of the object it is currently protecting.
        const MAX_NON_FREE_HAZARDS: usize = 128;

        // Push the given hazard to the cache.
        self.available_hazards.push(hazard);

        // Check if we exceeded the limit.
        if self.non_free_hazards() > MAX_NON_FREE_HAZARDS {
            // We did; we must now set the non-free hazards to "free".
            for i in &self.available_hazards[self.available_hazards_free_before..] {
                i.set(hazard::State::Free);
            }

            // Update the counter such that we mark the new hazards set to "free".
            self.available_hazards_free_before = self.available_hazards.len();
        }
    }

    /// See `add_garbage()`.
    fn add_garbage(&mut self, garbage: Garbage) {
        /// The maximal amount of garbage before exportation to the global state.
        const MAX_GARBAGE: usize = 128;

        // Push the garbage to the cache of garbage.
        self.garbage.push(garbage);

        // Export the garbage if it exceeds the limit.
        // TODO: use memory instead of items as a metric.
        if self.garbage.len() > MAX_GARBAGE {
            self.export_garbage_and_tick();
        }
    }

    /// See `export_garbage()`.
    fn export_garbage(&mut self) {
        // Clear the vector and export the garbage.
        global::export_garbage(mem::replace(&mut self.garbage, Vec::new()));
    }

    /// Export garbage (see `export_garbage()`), then tick.
    fn export_garbage_and_tick(&mut self) {
        self.export_garbage();
        global::tick();
    }
}

impl Drop for State {
    fn drop(&mut self) {
        // The thread is exiting, thus we must export the garbage to the global state to avoid
        // memory leaks. It is very important that this does indeed not tick, as causing garbage
        // collection means accessing RNG state, a TLS variable, which cannot be done when, we are
        // here, after it has deinitialized.
        self.export_garbage();

        // Clear every hazard to "dead" state.
        for hazard in self.available_hazards.drain(..) {
            hazard.kill();
        }
    }
}
