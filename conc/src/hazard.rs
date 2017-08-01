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

use std::sync::atomic::{self, AtomicPtr};
use std::{mem, thread};

use {debug, local};

/// Pointers to this represents the blocked state.
static BLOCKED: u8 = 0;
/// Pointers to this represents the free state.
static FREE: u8 = 0;
/// Pointers to this represents the dead state.
static DEAD: u8 = 0;

/// The state of a hazard.
///
/// Note that this `enum` excludes the blocked state, because it is semantically different from the
/// other states.
#[derive(PartialEq, Debug)]
#[must_use = "Hazard states are expensive to fetch and have no value unless used."]
pub enum State {
    /// The hazard does not currently protect any object.
    Free,
    /// The hazard is dead and may be deallocated when necessary.
    ///
    /// When a hazard has enetered this state, it shouldn't be used further. For example, you
    /// shouldn't change the state or alike, as that is not necessarily defined behavior as it can
    /// have been deallocated.
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

/// Create a new hazard reader-writer pair.
///
/// This creates a new hazard pair in blocked state.
///
/// Both ends of the hazards holds a shared reference to the state of the hazard. It represents the
/// same as `State` but is encoded such that it is atomically accessible.
///
/// Furthermore, there is an additional state: Blocked. If the hazard is in this state, reading it
/// will block until it no longer is. This is useful for blocking garbage collection while a value
/// is being read (avoiding the ABA problem).
pub fn create() -> (Writer, Reader) {
    // Allocate the hazard on the heap.
    let ptr = unsafe {
        &*Box::into_raw(Box::new(AtomicPtr::new(&BLOCKED as *const u8 as *mut u8)))
    };

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
    ptr: &'static AtomicPtr<u8>,
}

impl Reader {
    /// Get the state of the hazard.
    ///
    /// It will spin until the hazard is no longer in a blocked state, unless it is in debug mode,
    /// where it will panic given enough spins.
    pub fn get(&self) -> State {
        // In debug mode, we count the number of spins. In release mode, this should be trivially
        // optimized out.
        let mut spins = 0;

        // Spin until not blocked.
        loop {
            let ptr = self.ptr.load(atomic::Ordering::Acquire) as *const u8;

            // Blocked means that the hazard is blocked by another thread, and we must loop until
            // it assumes another state.
            if ptr == &BLOCKED {
                // Increment the number of spins.
                spins += 1;
                debug_assert!(spins < 100_000_000, "\
                    Hazard blocked for 100 millions rounds. Panicking as chances are that it will \
                    never get unblocked.\
                ");

                continue;
            } else if ptr == &FREE {
                return State::Free;
            } else if ptr == &DEAD {
                return State::Dead;
            } else {
                return State::Protect(ptr);
            }
        }
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
        Box::from_raw(self.ptr as *const AtomicPtr<u8> as *mut AtomicPtr<u8>);
        // Ensure that the RAII destructor doesn't kick in and crashes the program.
        mem::forget(self);
    }
}

/// Panic when it is dropped outside `Reader::destroy()`.
///
/// This ought to catch e.g. unwinding.
impl Drop for Reader {
    fn drop(&mut self) {
        panic!("\
            Hazard readers ought to be destroyed manually through the `Reader::destroy()` method.\
        ");
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
    ptr: &'static AtomicPtr<u8>,
}

impl Writer {
    /// Is the hazard blocked?
    pub fn is_blocked(&self) -> bool {
        self.ptr.load(atomic::Ordering::Acquire) as *const u8 == &BLOCKED
    }

    /// Block the hazard.
    pub fn block(&self) {
        self.ptr.store(&BLOCKED as *const u8 as *mut u8, atomic::Ordering::Release);
    }

    /// Set the hazard to "free".
    ///
    /// This sets the state to `State::Free`.
    pub fn free(&self) {
        self.ptr.store(&FREE as *const u8 as *mut u8, atomic::Ordering::Release);
    }

    /// Protect a pointer with the hazard.
    ///
    /// This sets the state to `State::Protect(ptr)` where `ptr` is the provided argument. Note
    /// that `ptr` can't be any of the internal reserved special-state pointer.
    pub fn protect(&self, ptr: *const u8) {
        debug::exec(|| println!("Protecting: 0x{:x}", ptr as usize));

        self.ptr.store(ptr as *mut u8, atomic::Ordering::Release);
    }

    /// Set the hazard to "dead".
    ///
    /// This sets the state to `State::Dead`.
    ///
    /// # Safety
    ///
    /// This is unsafe as usage after this has been called is breaking invariants. Use
    /// `Writer::kill()` to ensure safety through the type system.
    unsafe fn dead(&self) {
        self.ptr.store(&DEAD as *const u8 as *mut u8, atomic::Ordering::Release);
    }

    /// Set the hazard to "dead".
    ///
    /// This sets the state to `State::Dead`.
    ///
    /// It is consuming to ensure that the caller doesn't accidentally use the hazard reader
    /// afterwards, causing undefined behavior.
    pub fn kill(self) {
        // Set the state to dead (this is safe as we ensure, by move, that it is not used
        // afterwards).
        unsafe { self.dead(); }
        // Avoid the RAII destructor.
        mem::forget(self);
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
            // "dead" and move on. Setting it to dead is safe, as Rust ensures that it is not used
            // after the destructor (i.e. this function).
            unsafe { self.dead(); }
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
    use std::{ptr, thread};

    #[test]
    fn set_get() {
        let (w, r) = create();
        assert!(w.is_blocked());

        w.free();
        assert!(!w.is_blocked());
        assert_eq!(r.get(), State::Free);
        w.free();
        assert!(!w.is_blocked());
        assert_eq!(r.get(), State::Free);

        let x = 2;

        w.protect(&x);
        assert_eq!(r.get(), State::Protect(&x));

        w.protect(ptr::null());
        assert_eq!(r.get(), State::Protect(ptr::null()));
        w.protect(0x1 as *const u8);
        assert_eq!(r.get(), State::Protect(0x1 as *const u8));

        w.kill();
        unsafe {
            r.destroy();
        }
    }

    #[test]
    fn hazard_pair() {
        let (w, r) = create();
        let x = 2;

        w.free();
        assert_eq!(r.get(), State::Free);
        w.protect(&x);
        assert_eq!(r.get(), State::Protect(&x));
        w.kill();
        assert_eq!(r.get(), State::Dead);

        unsafe {
            r.destroy();
        }
    }

    #[test]
    fn cross_thread() {
        for _ in 0..64 {
            let (w, r) = create();

            thread::spawn(move || {
                w.kill();
            }).join().unwrap();

            assert_eq!(r.get(), State::Dead);
            unsafe { r.destroy(); }
        }
    }

    #[test]
    fn drop() {
        for _ in 0..9000 {
            let (w, r) = create();
            w.kill();
            unsafe {
                r.destroy();
            }
        }
    }

    /* FIXME: These tests are broken as the unwinding calls dtor of `Writer`, which double panics.
        #[cfg(debug_assertions)]
        #[test]
        #[should_panic]
        fn debug_infinite_blockage() {
            let (w, r) = create();
            let _ = r.get();

            w.kill();
            unsafe { r.destroy(); }
        }

        #[cfg(debug_assertions)]
        #[test]
        #[should_panic]
        fn debug_premature_free() {
            let (writer, reader) = create();
            writer.set(State::Free);
            mem::forget(reader);
            unsafe {
                reader.destroy();
            }
        }
    */
}
