//! Concurrent, atomic options.

use std::{mem, ptr};
use std::sync::atomic::{self, AtomicPtr};

use local;
use garbage::Garbage;
use guard::Guard;

/// A concurrently accessible and updatable optional pointer.
///
/// This acts as a kind of concurrent `Option<T>`.  It can be compared to `std::cell::RefCell` in
/// some ways: It allows accessing, referencing, updating, etc., however contrary to `RefCell`,
/// this is concurrent and has no aliasing restrictions. It is futher distinguished from
/// `std::sync::AtomicPtr` in that it allows references to the inner data without the ABA problem
/// or any variant thereof.
///
/// It conviniently wraps this crates API in a seemless manner.
pub struct AtomicOption<T> {
    /// The inner atomic pointer.
    inner: AtomicPtr<T>,
}

impl<T> AtomicOption<T> {
    /// Get a reference to the current content of the option.
    ///
    /// This returns a `Guard<T>`, which "protects" the inner value such that it is not dropped
    /// before the guard is no longer active. This is all handled automatically through RAII.
    ///
    /// The `ordering` defines what constraints the atomic operation has. Refer to the LLVM
    /// documentation for more information.
    pub fn load(&self, ordering: atomic::Ordering) -> Option<Guard<T>> {
        // Load the inner and wrap it in a guard.
        Guard::maybe_new(|| unsafe {
            self.inner.load(ordering).as_ref()
        })
    }

    /// Store a new value in the option.
    ///
    /// The old value of `self` will eventually be dropped, at some point after all the guarding
    /// references are gone.
    ///
    /// The `ordering` defines what constraints the atomic operation has. Refer to the LLVM
    /// documentation for more information.
    pub fn store(&self, new: Option<Box<T>>, ordering: atomic::Ordering) {
        // Transform the optional box to a (possibly null) pointer.
        // TODO: Use coercions.
        let new = new.map_or(ptr::null_mut(), |new| Box::into_raw(new));
        // Swap the contents with the new value.
        let ptr = self.inner.swap(new, ordering);
        if !ptr.is_null() {
            // Queue the deletion of the content.
            local::add_garbage(unsafe { Garbage::new_box(ptr) });
        }
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
    pub fn swap(&self, new: Option<Box<T>>, ordering: atomic::Ordering) -> Option<Guard<T>> {
        // Convert `new` into a raw pointer.
        // TODO: Use coercions.
        let new_ptr = new.map_or(ptr::null_mut(), Box::into_raw);

        // Create the guard. It is very important that this is done before the garbage is added,
        // otherwise we might introduce premature frees.
        Guard::maybe_new(|| unsafe {
            // Swap the atomic pointer with the new one.
            self.inner.swap(new_ptr, ordering).as_ref()
        }).map(|guard| {
            // Since the pointer is now unreachable from the option, it can safely be queued for
            // deletion.
            local::add_garbage(unsafe { Garbage::new_box(&*guard) });

            guard
        })
    }

    /// Store a value if the current matches a particular value.
    ///
    /// This compares `self` to `old`. If they match, the value is set to `new` and `Ok(())` is
    /// returned. Otherwise, `Err(new)` is returned.
    ///
    /// The `ordering` defines what constraints the atomic operation has. Refer to the LLVM
    /// documentation for more information.
    pub fn compare_and_store(&self, old: Option<&T>, mut new: Option<Box<T>>, ordering: atomic::Ordering)
    -> Result<(), Option<Box<T>>> {
        // Convert the paramteres to raw pointers.
        // TODO: Use coercions.
        let new_ptr = new.as_mut().map_or(ptr::null_mut(), |x| &mut **x);
        let old_ptr = old.map_or(ptr::null_mut(), |x| x as *const T as *mut T);

        // Compare-and-swap the value.
        let ptr = self.inner.compare_and_swap(old_ptr, new_ptr, ordering);

        // Check if the CAS was successful.
        if ptr == old_ptr {
            // It was. `self` is now `new`.

            // Ensure that the destructor of `new` is not run.
            mem::forget(new);

            // Queue the deletion of now-unreachable `old` (unless it's `None`).
            if !old_ptr.is_null() {
                local::add_garbage(unsafe { Garbage::new_box(old_ptr) });
            }

            Ok(())
        } else {
            // It failed.
            Err(new)
        }
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
    pub fn compare_and_swap(&self, old: Option<&T>, mut new: Option<Box<T>>, ordering: atomic::Ordering)
    -> Result<Option<Guard<T>>, (Option<Guard<T>>, Option<Box<T>>)> {
        // Convert the paramteres to raw pointers.
        // TODO: Use coercions.
        let new_ptr = new.as_mut().map_or(ptr::null_mut(), |x| &mut **x);
        let old_ptr = old.map_or(ptr::null_mut(), |x| x as *const T as *mut T);

        // Create the guard beforehand to avoid premature frees.
        let guard = Guard::maybe_new(|| {
            // The guard is active, so we can do the CAS now.
            unsafe { self.inner.compare_and_swap(old_ptr, new_ptr, ordering).as_ref() }
        });

        // Convert the guard to a raw pointer.
        // TODO: Use coercions.
        let guard_ptr = guard.as_ref().map_or(ptr::null_mut(), |x| &**x as *const T as *mut T);

        // Check if the CAS was successful.
        if guard_ptr == old_ptr {
            // It was. `self` is now `new`.

            // Ensure that the destructor of `new` is not run.
            mem::forget(new);

            // Queue the deletion of now-unreachable `old` (unless it's `None`).
            if !old_ptr.is_null() {
                local::add_garbage(unsafe { Garbage::new_box(old_ptr) });
            }

            Ok(guard)
        } else {
            // It failed; cast the raw pointer back to a box and return.
            Err((guard, new))
        }
    }
}
