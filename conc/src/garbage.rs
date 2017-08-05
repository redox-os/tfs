//! Literal garbage.

use debug;

/// An object to be deleted eventually.
///
/// Garbage refers to objects which are waiting to be destroyed, at some point after all references
/// to them are gone.
///
/// When it's dropped, the destructor of the garbage runs.
///
/// See also: ideology.
#[derive(Debug)]
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
        debug_assert!(ptr as usize > 0, "Creating garbage with invalid pointer.");

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
    // FIXME: This might actually be unsound, as it takes `T` and runs its destructor potentially
    //        in another thread. In other words, an (unaliased) `&mut T` is available in another
    //        thread through the destructor, meaning that it should be `Sync`, I think. I can't
    //        however think of any cases where this would lead to safety issues, but I think it is
    //        theoretically unsound. Investigate further.
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
}

impl Drop for Garbage {
    fn drop(&mut self) {
        // Print message in debug mode.
        debug::exec(|| println!("Destroying garbage: {:?}", self));

        unsafe { (self.dtor)(self.ptr); }
    }
}

// We must do this manually due to the raw pointer.
unsafe impl Send for Garbage {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    fn nop(_: *const u8) {}

    #[test]
    fn ptr() {
        let g = Garbage::new(0x2 as *const u8, nop);
        assert_eq!(g.ptr() as usize, 2);
    }

    #[test]
    fn new_box() {
        for _ in 0..1000 {
            unsafe { Garbage::new_box(Box::into_raw(Box::new(2))); }
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn debug_invalid_pointer() {
        Garbage::new(ptr::null(), nop);
    }
}
