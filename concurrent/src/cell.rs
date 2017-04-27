use std::mem;
use std::sync::atomic::{self, AtomicPtr};

use local;
use garbage::Garbage;
use guard::Guard;

/// A concurrently accessible and updatable cell.
///
/// This can be compared to `std::cell::RefCell` in some ways: It allows accessing, referencing,
/// updating, etc., however contrary to `RefCell`, this is concurrent and has no aliasing
/// restrictions. It is futher distinguished from `std::sync::AtomicPtr` in that it allows
/// references to the inner data without the ABA problem or any variant thereof.
///
/// It conviniently wraps this crates API in a seemless manner.
pub struct Cell<T> {
    /// The inner atomic pointer.
    inner: AtomicPtr<T>,
}

impl<T> Cell<T> {
    /// Get a reference to the current content of the cell.
    ///
    /// This returns a `Guard<T>`, which "protects" the inner value such that it is not dropped
    /// before the guard is no longer active. This is all handled automatically through RAII.
    ///
    /// The `ordering` defines what constraints the atomic operation has. Refer to the LLVM
    /// documentation for more information.
    pub fn load(&self, ordering: atomic::Ordering) -> Guard<T> {
        // Load the inner and wrap it in a guard.
        Guard::new(|| unsafe { &*self.inner.load(ordering) });
    }

    /// Store a new value in the cell.
    ///
    /// The old value of the cell will eventually be dropped, at some point after all the guarding
    /// references are gone.
    ///
    /// The `ordering` defines what constraints the atomic operation has. Refer to the LLVM
    /// documentation for more information.
    pub fn store(&self, new: Box<T>, ordering: atomic::Ordering) {
        // Swap the contents with the new value.
        let ptr = self.inner.swap(&new, ordering);
        // Queue the deletion of the content.
        local::add_garbage(unsafe { Garbage::new(ptr) });
    }

    /// Swap the old value with a new.
    ///
    /// This returns a `Guard<T>` as readers of the old values might exist. The old value will be
    /// queued for destruction.
    ///
    /// The `ordering` defines what constraints the atomic operation has. Refer to the LLVM
    /// documentation for more information.
    ///
    /// # Performance
    ///
    /// This is slower than `store` as it requires initializing a new guard, which requires at
    /// least two atomic operations. Thus, when possible, you should use `store`.
    pub fn swap(&self, new: Box<T>, ordering: atomic::Ordering) -> Guard<T> {
        // Create the guard. It is very important that this is done before the garbage is added,
        // otherwise we might introduce premature frees.
        let guard = Guard::new(|| {
            // Swap the atomic pointer with the new one.
            unsafe { &*self.inner.swap(new, ordering) }
        });

        // Since the pointer is now unreachable from the cell, it can safely be queued for
        // deletion.
        local::add_garbage(Garbage::new(&guard));

        guard
    }

    /// Swap a value if it matches.
    ///
    /// This compares `self` to `old`. If they match, it is swapped with `new` and a guard to the
    /// old value is returned wrapped in `Ok`. If not, a tuple containing the guard to the actual
    /// (non-matching) value and the box of `new` — wrapped in `Err` — is returned.
    ///
    /// The `ordering` defines what constraints the atomic operation has. Refer to the LLVM
    /// documentation for more information.
    ///
    /// # Performance
    ///
    /// This is slower than `compare_and_set` as it requires initializing a new guard, which
    /// requires at least two atomic operations. Thus, when possible, you should use
    /// `compare_and_set`.
    pub fn compare_and_swap(&self, old: &T, new: Box<T>, ordering: atomic::Ordering)
    -> Result<Guard<T>, (Guard<T>, Box<T>)> {
        // Create the guard beforehand to avoid premature frees.
        let guard = Guard::new(|| {
            // The guard is active, so we can do the CAS now.
            unsafe { &*self.inner.compare_and_swap(old, &new, ordering) }
        });

        // Check if the CAS was successful.
        if guard == old {
            // It was. `self` is now `new`.

            // This is critical for avoiding premature drop as the pointer to the box is stored in
            // `self.inner` now.
            mem::forget(new);

            // Queue the now-unreachable garbage of `old`.
            local::add_garbage(Garbage::new(old));

            Ok(guard)
        } else {
            // It failed.
            Err((guard, new))
        }
    }
}
