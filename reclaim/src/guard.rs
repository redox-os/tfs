struct Guard<T> {
    hazard: Option<hazard::Writer>,
    pointer: &'static T,
}

impl<T> Guard<T> {
    fn new<F>(ptr: F) -> Guard<T>
    where F: FnOnce() -> &'static T {
        let hazard = local::get_hazard();
        let ptr = ptr();
        hazard.set(hazard::State::Protect(ptr));
        Guard {
            hazard: hazard,
            pointer: ptr,
        }
    }

    // TODO: Is this sound?
    fn map<U, F>(self, f: F) -> Guard<U>
    where F: FnOnce(&T) -> &U {
        Guard {
            hazard: self.hazard,
            pointer: f(self.pointer),
        }
    }
}

impl<T> Drop for Guard<T> {
    fn drop(&mut self) {
        local::free_hazard(self.hazard.take().unwrap());
    }
}

impl<T> ops::Deref for Guard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.pointer }
    }
}
