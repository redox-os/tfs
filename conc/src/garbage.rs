//! Literal garbage.

use std::mem;

/// An object to be deleted eventually.
///
/// Garbage refers to objects which are waiting to be destroyed, at some point after all references
/// to them are gone.
///
/// Refer to `Garbage::destroy()` for details on destruction.
///
/// See also: ideology.
pub struct Garbage {
    /// The pointer to the object.
    ptr: *const u8,
    /// The destructor of the object.
    ///
    /// The argument given when called is the `self.ptr` field.
    dtor: unsafe fn(*const u8),
}

impl Garbage {
    /// Create a new garbage item given its parameters.
    ///
    /// This takes the pointer and destructor (which takes pointer as argument) and construct the
    /// corresponding garbage item.
    pub fn new(ptr: *const u8, dtor: fn(*const u8)) -> Garbage {
        // TODO: Add assertion against null pointers.

        Garbage {
            ptr: ptr,
            dtor: dtor,
        }
    }

    /// Create a garbage item deallocating and dropping a box.
    ///
    /// Assuming `item` is a pointer representing a `Box`, this creates a garbage item, which has
    /// a destructor dropping and deallocating the box represented by `item`.
    ///
    /// Due to the affine type system, we must pass a pointer rather than the box directly.
    ///
    /// # Safety
    ///
    /// This is unsafe as there is no way to verify that `item` is indeed a box, nor is it possible
    /// to secure against double-drops and other issues arising from the fact that we're passing a
    /// pointer.
    // TODO: Find a way to do this safely.
    pub unsafe fn new_box<T>(item: *const T) -> Garbage {
        unsafe fn dtor<T>(ptr: *const u8)  {
            // Drop the box represented by `ptr`.
            Box::from_raw(ptr as *mut u8 as *mut T);
        }

        Garbage {
            ptr: item as *const u8,
            dtor: dtor::<T>,
        }
    }

    /// Get the inner pointer of the garbage.
    pub fn ptr(&self) -> *const u8 {
        self.ptr
    }

    /// Destroy the garbage.
    ///
    /// This runs the destructor associated with the data. That is, it runs the destructor function
    /// pointer with the provided data pointer as argument.
    ///
    /// # Panic
    ///
    /// This function should never unwind, even if the destructor does. In particular, any
    /// unwinding causes a safe crash, equivalent to double-panicking (i.e. SIGILL). This ought to
    /// avoid spurious unwinding through unrelated stacks and messing with the environment within
    /// the system.
    pub fn destroy(self) {
        // TODO: Let this unwind by fixing the bugs in `global`.

        /// Stop any unwinding.
        ///
        /// This struct stops unwinding through it by double-panicking in its destructor, thus
        /// safely SIGILL-ing the program. It is meant to avoid unwinding.
        struct StopUnwind;

        impl Drop for StopUnwind {
            fn drop(&mut self) {
                panic!("Panicking during unwinding to stop unwinding.");
            }
        }

        let guard = StopUnwind;
        // Run, but catch any panicks that the dtor might cause.
        unsafe { (self.dtor)(self.ptr); }
        // Prevent the guard's destructor from running.
        mem::forget(guard);
    }
}

// We must do this manually due to the raw pointer.
unsafe impl Send for Garbage {}
