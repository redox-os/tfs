struct Garbage {
    ptr: *const u8,
    dtor: unsafe fn(*const u8),
}

impl Garbage {
    fn new<T>(item: Box<T>) -> Garbage {
        unsafe fn dtor<T>(ptr: *const u8)  {
            drop(Box::from_raw(ptr as *mut u8 as *const T));
        }

        Garbage {
            ptr: &item,
            dtor: dtor::<T>,
        }
    }

    fn destroy(self) {
        unsafe { self.dtor(self.ptr); }
    }
}
