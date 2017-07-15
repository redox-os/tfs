//! Hazard pointer management.
//!
//! "Hazard" is the general name for an atomic pointer potentially "protecting" some object from
//! getting deleted. First when the hazard is changed to another state, the object can be deleted.
//!
//! Since hazards are only useful when they're shared between the global and local state, a "hazard
//! pair" refers to the collection of the two connected ends of the hazard (they both share a
//! pointer to the hazard on the heap). When such pair is created, its reading end is usually
//! stored in the global state, such that a thread can check that no hazard is matching a
//! particular object during garbage collection. The writer part controls what the state of the
//! hazard is and is usually passed around locally.
//!
//! The asymmetry of a hazard pair is strictly speaking not necessary, but it allows to enforce
//! rules (e.g. only the reader/global part may deallocate the hazard box).

use std::sync::atomic::{self, AtomicIsize};
use std::{ops, mem, thread};

use local;

/// The state of a hazard.
///
/// Note that this `enum` excludes the blocked state, because it is semantically different from the
/// other states.
#[derive(PartialEq, Debug)]
pub enum State {
    /// The hazard does not currently protect any object.
    Free,
    /// The hazard is dead and may be deallocated when necessary.
    Dead,
    /// The hazard is protecting an object.
    ///
    /// "Protecting" means that the pointer it holds is not deleted while the hazard is in this
    /// state.
    ///
    /// The inner pointer is restricted to values not overlapping with the trap value,
    /// corresponding to one of the other states.
    Protect(*const u8),
}

/// A hazard.
///
/// This type holds an atomic pointer with the state of the hazard. It represents the same as
/// `State` but is encoded such that it is atomically accessible.
///
/// Furthermore, there is an additional state: Blocked. If the hazard is in this state, reading it
/// will block until it no longer is. This is useful for blocking garbage collection while a value
/// is being read (avoiding the ABA problem).
#[derive(Debug)]
pub struct Hazard {
    /// The inner atomic value.
    ///
    /// This takes following forms:
    ///
    /// - -1: blocked.
    /// - -2: free.
    /// - -3: dead
    /// - otherwise: protecting the address represented by the `usize`.
    ptr: AtomicIsize,
}

impl Hazard {
    /// Create a new hazard in blocked state.
    pub fn blocked() -> Hazard {
        Hazard {
            ptr: AtomicIsize::new(-1),
        }
    }

    /// Block the hazard.
    pub fn block(&self) {
        self.ptr.store(-1, atomic::Ordering::Release);
    }

    /// Set the hazard to a new state.
    ///
    /// Whether or not it is blocked has no effect on this. To get it back to the blocked state,
    /// use `self.block()`.
    pub fn set(&self, new: State) {
        // Simply encode and store.
        self.ptr.store(match new {
            State::Free => -2,
            State::Dead => -3,
            State::Protect(ptr) => {
                debug_assert!(!(-3..0).contains(ptr as isize), "Protecting an invalid pointer, \
                colliding with a trap value of the hazard. This likely happens because you have \
                stored or corrupted the pointer in a container. Ensure that you only store valid \
                values in hazards.");
                ptr as isize
            }
        }, atomic::Ordering::Release);
    }

    /// Get the state of the hazard.
    ///
    /// It will spin until the hazard is no longer in a blocked state.
    pub fn get(&self) -> State {
        // Spin until not blocked.
        loop {
            return match self.ptr.load(atomic::Ordering::Acquire) {
                // -1 means that the hazard is blocked by another thread, and we must loop until it
                // assumes another state.
                -1 => continue,
                -2 => State::Free,
                -3 => State::Dead,
                ptr => State::Protect(ptr as *const u8)
            };
        }
    }
}

/// Create a new hazard reader-writer pair.
///
/// This creates a new hazard pair in blocked state.
pub fn create() -> (Writer, Reader) {
    // Allocate the hazard on the heap.
    let ptr: &'static Hazard = unsafe { &*Box::into_raw(Box::new(Hazard::blocked())) };

    // Construct the values.
    (Writer {
        ptr: ptr,
    }, Reader {
        ptr: ptr,
    })
}

/// An hazard reader.
///
/// This wraps a hazard and provides only ability to read and deallocate it. It is created through
/// the `create()` function.
///
/// The destructor will, for the sake of safety, panic. To deallocate, use `self.destroy()`
/// instead.
pub struct Reader {
    /// The pointer to the heap-allocated hazard.
    ptr: &'static Hazard,
}

impl Reader {
    /// Get the state of the hazard.
    pub fn get(&self) -> State {
        self.ptr.get()
    }

    /// Destroy the hazard.
    ///
    /// # Safety
    ///
    /// This is unsafe as it relies on the writer part being dead and not used anymore. There is
    /// currently no way to express this invariant through the type system, so we must rely on the
    /// caller to ensure that.
    ///
    /// # Panics
    ///
    /// In debug mode, this will panic if the state of the hazard is not "dead".
    pub unsafe fn destroy(self) {
        debug_assert!(self.get() == State::Dead, "Prematurely freeing an active hazard.");

        // Load the pointer and deallocate it.
        Box::from_raw(self.ptr as *const Hazard as *mut Hazard);
        // Ensure that the RAII destructor doesn't kick in and crashes the program.
        mem::forget(self);
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        panic!("Hazard readers ought to be destroyed manually through the `destroy` method.");
    }
}

/// An hazard reader.
///
/// This wraps a hazard and provides only ability to read and deallocate it. It is created through
/// the `create()` function.
///
/// The destructor relocate the hazard to the thread-local cache.
#[derive(Debug)]
pub struct Writer {
    /// The pointer to the heap-allocated hazard.
    ptr: &'static Hazard,
}

impl Writer {
    /// Set the state of this hazard to "dead".
    ///
    /// This will ensure that the hazard won't end up in the thread-local cache, by (eventually)
    /// deleting it globally.
    ///
    /// Generally, this is not recommended, as it means that your hazard cannot be reused.
    pub fn kill(self) {
        // Set the state to dead.
        self.set(State::Dead);
        // Avoid the RAII destructor.
        mem::forget(self);
    }
}

impl ops::Deref for Writer {
    type Target = Hazard;

    fn deref(&self) -> &Hazard {
        self.ptr
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // Implementation note: Freeing to local state in the destructor does lead to issues with
        // panicking, which this conditional is supposed to solve. The alternative is to outright
        // set the hazard to state "dead", disregarding whether the thread is panicking or not.
        // This is fairly nice for some purposes, but it makes it necessary to store an `Option` in
        // `Guard`, as one must avoid the hazard from being set to state "dead" after being
        // relocated to the local state. As such, this approach (where the destructor automatically
        // puts the hazard back into the local cache) is nicer. For more information on its
        // alternative, see commit b7047c263cbd614b7c828d68b29d7928be543623.
        if thread::panicking() {
            // If the thread is unwinding, there is no point in putting it back in the thread-local
            // cache. In fact, it might cause problems, if the unwinding tries to garbage collect
            // and the hazard is in blocked state. For this reason, we simply set the state to
            // "dead" and move on.
            self.ptr.set(State::Dead);
        } else {
            // Free the hazard to the thread-local cache. We have to clone the hazard to get around the
            // fact that `drop` takes `&mut self`.
            local::free_hazard(Writer {
                ptr: self.ptr,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn hazard_set_get() {
        let h = Hazard::blocked();
        h.set(State::Free);
        assert_eq!(h.get(), State::Free);
        h.set(State::Dead);
        assert_eq!(h.get(), State::Dead);

        let x = 2;

        h.set(State::Protect(&x));
        assert_eq!(h.get(), State::Protect(&x));

        h.set(State::Protect(ptr::null()));
        assert_eq!(h.get(), State::Protect(ptr::null()));
        h.set(State::Protect(0x1 as *const u8));
        assert_eq!(h.get(), State::Protect(0x1 as *const u8));
    }

    #[test]
    fn hazard_pair() {
        let (writer, reader) = create();
        let x = 2;

        writer.set(State::Free);
        assert_eq!(reader.get(), State::Free);
        writer.set(State::Protect(&x));
        assert_eq!(reader.get(), State::Protect(&x));
        writer.kill();
        assert_eq!(reader.get(), State::Free);

        unsafe {
            reader.destroy();
        }
    }

    #[test]
    fn drop() {
        for _ in 0..9000 {
            let (writer, reader) = create();
            writer.set(State::Free);
            unsafe {
                reader.destroy();
            }
        }
    }

    // TODO: More tests here.
}
