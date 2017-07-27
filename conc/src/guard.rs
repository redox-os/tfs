//! RAII guards for hazards.

use std::{cmp, ops};
use std::sync::atomic;
use {hazard, local};

#[cfg(debug_assertions)]
use std::cell::Cell;
#[cfg(debug_assertions)]
thread_local! {
    /// Number of guards the current thread is creating.
    static CURRENT_CREATING: Cell<usize> = Cell::new(0);
}

/// Assert (in debug mode) that no guards are currently being created in this thread.
///
/// This shall be used when you want to ensure, that a function called within the guard constructor
/// doesn't cause endless looping, due to the blocked hazard.
///
/// In particular, it should be called in functions that could trigger a garbage collection, thus
/// requiring that hazards are eventually unblocked.
pub fn debug_assert_no_create() {
    #[cfg(debug_assertions)]
    CURRENT_CREATING.with(|x| assert_eq!(x.get(), 0));
}

/// A RAII guard protecting from garbage collection.
///
/// This "guards" the held pointer against garbage collection. First when all guards of said
/// pointer is gone (the data is unreachable), it can be collected.
// TODO: Remove this `'static` bound.
#[must_use = "\
    You are getting a `conc::Guard<T>` without using it, which means it is potentially \
    unnecessary overhead. Consider replacing the method with something that doesn't \
    return a guard.\
"]
#[derive(Debug)]
pub struct Guard<T: 'static + ?Sized> {
    /// The inner hazard.
    hazard: hazard::Writer,
    /// The pointer to the protected object.
    pointer: &'static T,
}

impl<T: ?Sized> Guard<T> {
    /// (Failably) create a new guard.
    ///
    /// This has all the same restrictions and properties as `Guard::new()` (please read its
    /// documentation before using), with the exception of being failable.
    ///
    /// This means that the closure can return and error and abort the creation of the guard.
    pub fn try_new<F, E>(ptr: F) -> Result<Guard<T>, E>
    where F: FnOnce() -> Result<&'static T, E> {
        // Increment the number of guards currently being created.
        #[cfg(debug_assertions)]
        CURRENT_CREATING.with(|x| x.set(x.get() + 1));

        // Get a hazard in blocked state.
        let hazard = local::get_hazard();

        // This fence is necessary for ensuring that `hazard` does not get reordered to after `ptr`
        // has run.
        // TODO: Is this fence even necessary?
        atomic::fence(atomic::Ordering::SeqCst);

        // Right here, any garbage collection is blocked, due to the hazard above. This ensures
        // that between the potential read in `ptr` and it being protected by the hazard, there
        // will be no premature free.

        // Evaluate the pointer through the closure.
        let res = ptr();

        // Decrement the number of guards currently being created.
        #[cfg(debug_assertions)]
        CURRENT_CREATING.with(|x| x.set(x.get() - 1));

        match res {
            Ok(ptr) => {
                // Now that we have the pointer, we can protect it by the hazard, unblocking a pending
                // garbage collection if it exists.
                hazard.protect(ptr as *const T as *const u8);

                Ok(Guard {
                    hazard: hazard,
                    pointer: ptr,
                })
            },
            Err(err) => {
                // Set the hazard to free to ensure that the hazard doesn't remain blocking.
                hazard.free();

                Err(err)
            }
        }
    }

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
        Guard::try_new::<_, ()>(|| Ok(ptr())).unwrap()
    }

    /// Conditionally create a guard.
    ///
    /// This acts `try_new`, but with `Option` instead of `Result`.
    pub fn maybe_new<F>(ptr: F) -> Option<Guard<T>>
    where F: FnOnce() -> Option<&'static T> {
        Guard::try_new(|| ptr().ok_or(())).ok()
    }

    /// Map the pointer to another.
    ///
    /// This allows one to map a pointer to a pointer e.g. to an object referenced by the old. It
    /// is very convenient for creating APIs without the need for creating a wrapper type.
    // TODO: Is this sound?
    pub fn map<U, F>(self, f: F) -> Guard<U>
    where F: FnOnce(&T) -> &U {
        Guard {
            hazard: self.hazard,
            pointer: f(self.pointer),
        }
    }

    /// Get the raw pointer of this guard.
    pub fn as_ptr(&self) -> *const T {
        self.pointer
    }
}

impl<T> cmp::PartialEq for Guard<T> {
    fn eq(&self, other: &Guard<T>) -> bool {
        self.as_ptr() == other.as_ptr()
    }
}

impl<T> cmp::Eq for Guard<T> {}

impl<T> ops::Deref for Guard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.pointer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    #[should_panic]
    fn panic_during_guard_creation() {
        let _ = Guard::new(|| -> &'static u8 { panic!() });
    }

    #[test]
    fn nested_guard_creation() {
        for _ in 0..100 {
            let _ = Guard::new(|| {
                mem::forget(Guard::new(|| "blah"));
                "blah"
            });
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn debug_catch_infinite_blockage() {
        let _ = Guard::new(|| {
            local::export_garbage();
            "blah"
        });
    }
}
