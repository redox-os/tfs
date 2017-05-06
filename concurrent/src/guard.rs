//! RAII guards for hazards.

use std::ops;
use {hazard, local};

/// A RAII guard protecting from garbage collection.
///
/// This "guards" the held pointer against garbage collection. First when all guards of said
/// pointer is gone (the data is unreachable), it can be colleceted.
// TODO: Remove this `'static` bound.
pub struct Guard<T: 'static> {
    /// The hazard.
    ///
    /// This is wrapped in an option to allow moving it out and back to the local state in the
    /// destructor — something that is impossible otherwise due to `drop` taking `&mut self`.
    // TODO: ^ Get rid of the option.
    hazard: Option<hazard::Writer>,
    /// The pointer to the protected object.
    pointer: &'static T,
}

impl<T> Guard<T> {
    /// Create a new guard.
    ///
    /// Because it must ensure that no garbage collection happens until the pointer is read, it
    /// takes a closure, which is evaluated to the pointer the guard will hold. During the span of
    /// this closure, garbage collection is ensured to not happen, making it safe to read from an
    /// atomic pointer without risking the ABA problem.
    ///
    /// # Important!
    ///
    /// It is very important that this closure does not contain anything which might cause a
    /// garbage collection, as garbage collecting inside this closure will cause the current thread
    /// to be blocked infinitely (because the hazard is blocked) and stop all other threads from
    /// collecting garbage, leading to memory leaks in those.
    pub fn new<F>(ptr: F) -> Guard<T>
    where F: FnOnce() -> &'static T {
        // Get a hazard in blocked state.
        let hazard = local::get_hazard();
        // Right here, any garbage collection is blocked, due to the hazard above. This ensures
        // that between the potential read in `ptr` and it being protected by the hazard, there
        // will be no premature free.

        // Evaluate the pointer under the pr
        let ptr = ptr();
        // Now that we have the pointer, we can protect it by the hazard, unblocking a pending
        // garbage collection if it exists.
        hazard.set(hazard::State::Protect(ptr as *const T as *const u8));

        Guard {
            hazard: Some(hazard),
            pointer: ptr,
        }
    }

    /// Map the pointer to another.
    ///
    /// This allows one to map a pointer to a pointer e.g. to an object referenced by the old. It
    /// is very convinient for creating APIs without the need for creating a wrapper type.
    // TODO: Is this sound?
    pub fn map<U, F>(mut self, f: F) -> Guard<U>
    where F: FnOnce(&T) -> &U {
        Guard {
            hazard: self.hazard.take(),
            pointer: f(self.pointer),
        }
    }
}

impl<T> Drop for Guard<T> {
    fn drop(&mut self) {
        // Put the hazard back to the local state for potential reuse.
        local::free_hazard(self.hazard.take().unwrap());
    }
}

impl<T> ops::Deref for Guard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.pointer
    }
}
