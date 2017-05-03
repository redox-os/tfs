//! Literal garbage.

/// An object to be deleted eventually.
///
/// Garbage refers to objects which are waiting to be destroyed, at some point after all references
/// to them are gone.
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
    fn new(ptr: *const u8, dtor: fn(*const u8)) -> Garbage {
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
    unsafe fn new_box<T>(item: *const T) -> Garbage {
        unsafe fn dtor<T>(ptr: *const u8)  {
            // Drop the box represented by `ptr`.
            Box::from_raw(ptr as *mut u8 as *mut T);
        }

        Garbage {
            ptr: item,
            dtor: dtor::<T>,
        }
    }

    pub fn destroy(self) {
        unsafe { self.dtor(self.ptr); }
    }
}

// We must do this manually due to the raw pointer.
unsafe impl Send for Garbage {}
