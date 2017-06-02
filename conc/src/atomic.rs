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
/// this is concurrent and has no aliasing restrictions. It is further distinguished from
/// `std::sync::AtomicPtr` in that it allows references to the inner data without the ABA problem
/// or any variant thereof.
///
/// It conveniently wraps this crates API in a seemless manner.
#[derive(Default)]
pub struct Atomic<T> {
    /// The inner atomic pointer.
    inner: AtomicPtr<T>,
}

impl<T> Atomic<T> {
    /// Create a new concurrent option.
    pub fn new(init: Option<Box<T>>) -> Atomic<T> {
        Atomic {
            // Convert the box to a raw pointer.
            inner: AtomicPtr::new(init.map_or(ptr::null_mut(), Box::into_raw)),
        }
    }

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

    /// Store a (raw) pointer if the current matches the specified pointer.
    ///
    /// This compares `self` to `old`. If they match, the value is set to `new` and `Ok(())` is
    /// returned. Otherwise, `Err(())` is returned.
    ///
    /// The `ordering` defines what constraints the atomic operation has. Refer to the LLVM
    /// documentation for more information.
    ///
    /// # Safety
    ///
    /// As this accepts a raw pointer, it is necessary to mark it as `unsafe`. To uphold the
    /// invariants, ensure that `new` isn't used after this function has been called.
    pub unsafe fn compare_and_store_raw(&self, old: *const T, new: *mut T, ordering: atomic::Ordering)
    -> Result<(), ()> {

        // Compare-and-swap the value and check if it was successful.
        if self.inner.compare_and_swap(old as *mut T, new, ordering) as *const T == old {
            // It was. `self` is now `new`.

            // Queue the deletion of now-unreachable `old` (unless it's `None`).
            if !old.is_null() {
                local::add_garbage(Garbage::new_box(old));
            }

            Ok(())
        } else {
            // It failed.
            Err(())
        }
    }

    /// Store a pointer if the current matches the specified pointer.
    ///
    /// This compares `self` to `old`. If they match, the value is set to `new` and `Ok(())` is
    /// returned. Otherwise, `Err(new)` is returned.
    ///
    /// The `ordering` defines what constraints the atomic operation has. Refer to the LLVM
    /// documentation for more information.
    pub fn compare_and_store(&self, old: Option<*const T>, new: Option<Box<T>>, ordering: atomic::Ordering)
    -> Result<(), Option<Box<T>>> {
        // Run the CAS.
        if unsafe {
            self.compare_and_store_raw(
                // Convert the input to raw pointers.
                old.unwrap_or(ptr::null()),
                // TODO: Does this break NOALIAS if it is enabled on `Box` in the future in `rustc`
                //       (see also `compare_and_swap`)?
                new.as_ref().map_or(ptr::null_mut(), |x| &**x as *const T as *mut T),
                ordering,
            )
        }.is_ok() {
            // `new` is now in `self`. We must thus ensure that the destructor isn't called, as
            // that might cause use-after-free.
            mem::forget(new);

            Ok(())
        } else {
            // Hand back the box.
            Err(new)
        }
    }

    /// Swap a (raw) pointer if it matches the specified pointer.
    ///
    /// This compares `self` to `old`. If they match, it is swapped with `new` and a guard to the
    /// old value is returned wrapped in `Ok`. If not, a tuple containing the guard to the actual
    /// (non-matching) value, wrapped in `Err()` is returned.
    ///
    /// The `ordering` defines what constraints the atomic operation has. Refer to the LLVM
    /// documentation for more information.
    ///
    /// # Safety
    ///
    /// As this accepts a raw pointer, it is necessary to mark it as `unsafe`. To uphold the
    /// invariants, ensure that `new` isn't used after this function has been called.
    pub unsafe fn compare_and_swap_raw(
        &self,
        old: *const T,
        new: *mut T,
        ordering: atomic::Ordering
    ) -> Result<Option<Guard<T>>, Option<Guard<T>>> {
        // Create the guard beforehand to avoid premature frees.
        let guard = Guard::maybe_new(|| {
            // The guard is active, so we can do the CAS now.
            self.inner.compare_and_swap(old as *mut T, new, ordering).as_ref()
        });

        // Convert the guard to a raw pointer.
        // TODO: Use coercions.
        let guard_ptr = guard.as_ref().map_or(ptr::null_mut(), |x| &**x as *const T as *mut T);

        // Check if the CAS was successful.
        if guard_ptr as *const T == old {
            // It was. `self` is now `new`.

            // Queue the deletion of now-unreachable `old` (unless it's `None`).
            if !old.is_null() {
                local::add_garbage(Garbage::new_box(old));
            }

            Ok(guard)
        } else {
            Err(guard)
        }
    }

    /// Swap a pointer if it matches the specified pointer.
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
    /// This is slower than `compare_and_store` as it requires initializing a new guard, which
    /// requires at least two atomic operations. Thus, when possible, you should use
    /// `compare_and_store`.
    pub fn compare_and_swap(&self, old: Option<*const T>, new: Option<Box<T>>, ordering: atomic::Ordering)
    -> Result<Option<Guard<T>>, (Option<Guard<T>>, Option<Box<T>>)> {
        // Run the CAS.
        match unsafe {
            self.compare_and_swap_raw(
                // Convert the input to raw pointers.
                old.unwrap_or(ptr::null()),
                new.as_ref().map_or(ptr::null_mut(), |x| &**x as *const T as *mut T),
                ordering,
            )
        } {
            Ok(guard) => {
                // `new` is now in `self`. We must thus ensure that the destructor isn't called, as
                // that might cause use-after-free.
                mem::forget(new);

                Ok(guard)
            },
            // Hand back the box too.
            Err(guard) => Err((guard, new))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{atomic, Arc};
    use std::sync::atomic::AtomicUsize;
    use std::{thread, ptr};

    struct Basic;

    impl Drop for Basic {
        fn drop(&mut self) {
            basic();
        }
    }

    thread_local! {
        static BASIC: Basic = Basic;
    }

    fn basic() {
        let opt = Atomic::default();
        assert!(opt.load(atomic::Ordering::Relaxed).is_none());
        assert!(opt.swap(None, atomic::Ordering::Relaxed).is_none());
        assert!(opt.load(atomic::Ordering::Relaxed).is_none());
        assert!(opt.swap(Some(Box::new(42)), atomic::Ordering::Relaxed).is_none());
        assert_eq!(*opt.load(atomic::Ordering::Relaxed).unwrap(), 42);
        assert_eq!(*opt.swap(Some(Box::new(43)), atomic::Ordering::Relaxed).unwrap(), 42);
        assert_eq!(*opt.load(atomic::Ordering::Relaxed).unwrap(), 43);
    }

    #[test]
    fn basic_properties() {
        basic()
    }

    #[test]
    fn cas() {
        let bx1 = Box::new(1);
        let ptr1 = &*bx1 as *const usize;
        let bx2 = Box::new(1);
        let ptr2 = &*bx2 as *const usize;

        let opt = Atomic::new(Some(bx1));
        assert_eq!(ptr1, &*opt.compare_and_swap(Some(ptr2), None, atomic::Ordering::Relaxed).unwrap_err().0.unwrap());
        assert_eq!(ptr1, &*opt.load(atomic::Ordering::Relaxed).unwrap());

        assert_eq!(ptr1, &*opt.compare_and_swap(None, Some(Box::new(2)), atomic::Ordering::Relaxed).unwrap_err().0.unwrap());
        assert_eq!(ptr1, &*opt.load(atomic::Ordering::Relaxed).unwrap());

        opt.compare_and_swap(Some(ptr1), None, atomic::Ordering::Relaxed).unwrap();
        assert!(opt.load(atomic::Ordering::Relaxed).is_none());

        opt.compare_and_swap(None, Some(bx2), atomic::Ordering::Relaxed).unwrap();
        assert_eq!(ptr2, &*opt.load(atomic::Ordering::Relaxed).unwrap());

        opt.compare_and_store(Some(ptr2), None, atomic::Ordering::Relaxed).unwrap();
        opt.compare_and_store(Some(Box::into_raw(Box::new(2))), None, atomic::Ordering::Relaxed).unwrap_err();

        assert!(opt.load(atomic::Ordering::Relaxed).is_none());

        // To check that GC doesn't segfault or something.
        ::gc();
        ::gc();
        ::gc();
        ::gc();
    }

    #[test]
    fn cas_raw() {
        unsafe {
            let bx1 = Box::new(1);
            let ptr1 = &*bx1 as *const usize;
            let bx2 = Box::new(1);
            let ptr2 = &*bx2 as *const usize;

            let opt = Atomic::new(Some(bx1));
            assert_eq!(ptr1, &*opt.compare_and_swap_raw(ptr2, ptr::null_mut(), atomic::Ordering::Relaxed).unwrap_err().unwrap());
            assert_eq!(ptr1, &*opt.load(atomic::Ordering::Relaxed).unwrap());

            assert_eq!(ptr1, &*opt.compare_and_swap_raw(ptr::null(), Box::into_raw(Box::new(2)), atomic::Ordering::Relaxed).unwrap_err().unwrap());
            assert_eq!(ptr1, &*opt.load(atomic::Ordering::Relaxed).unwrap());

            opt.compare_and_swap_raw(ptr1, ptr::null_mut(), atomic::Ordering::Relaxed).unwrap();
            assert!(opt.load(atomic::Ordering::Relaxed).is_none());

            opt.compare_and_swap_raw(ptr::null(), Box::into_raw(bx2), atomic::Ordering::Relaxed).unwrap();
            assert_eq!(ptr2, &*opt.load(atomic::Ordering::Relaxed).unwrap());

            opt.compare_and_store_raw(ptr2, ptr::null_mut(), atomic::Ordering::Relaxed).unwrap();
            opt.compare_and_store_raw(Box::into_raw(Box::new(2)), ptr::null_mut(), atomic::Ordering::Relaxed).unwrap_err();

            assert!(opt.load(atomic::Ordering::Relaxed).is_none());

            // To check that GC doesn't segfault or something.
            ::gc();
            ::gc();
            ::gc();
            ::gc();
        }
    }

    #[test]
    fn spam() {
        let opt = Arc::new(Atomic::default());

        let mut j = Vec::new();
        for _ in 0..16 {
            let opt = opt.clone();
            j.push(thread::spawn(move || {
                for i in 0..1_000_001 {
                    let _ = opt.load(atomic::Ordering::Relaxed);
                    opt.store(Some(Box::new(i)), atomic::Ordering::Relaxed);
                }
                opt
            }))
        }

        ::gc();
        ::gc();

        for i in j {
            i.join().unwrap();
        }

        assert_eq!(*opt.load(atomic::Ordering::Relaxed).unwrap(), 1_000_000);
    }

    #[test]
    fn drop() {
        #[derive(Clone)]
        struct Dropper {
            d: Arc<AtomicUsize>,
        }

        impl Drop for Dropper {
            fn drop(&mut self) {
                self.d.fetch_add(1, atomic::Ordering::Relaxed);
            }
        }

        let drops = Arc::new(AtomicUsize::default());
        let opt = Arc::new(Atomic::new(None));

        let d = Dropper {
            d: drops.clone(),
        };

        let mut j = Vec::new();
        for _ in 0..16 {
            let d = d.clone();
            let opt = opt.clone();

            j.push(thread::spawn(move || {
                for _ in 0..1_000_000 {
                    opt.store(Some(Box::new(d.clone())), atomic::Ordering::Relaxed);
                }
            }))
        }

        for i in j {
            i.join().unwrap();
        }

        opt.store(None, atomic::Ordering::Relaxed);

        ::gc();

        // The 16 are for the `d` variable in the loop above.
        assert_eq!(drops.load(atomic::Ordering::Relaxed), 16_000_000 + 16);
    }

    #[test]
    fn tls() {
        thread::spawn(|| BASIC.with(|_| {})).join().unwrap();
        thread::spawn(|| BASIC.with(|_| {})).join().unwrap();
        thread::spawn(|| BASIC.with(|_| {})).join().unwrap();
        thread::spawn(|| BASIC.with(|_| {})).join().unwrap();
    }
}
