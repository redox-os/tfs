struct Atomic<T> {
    inner: AtomicPtr<T>,
}

impl<T> Atomic<T> {
    pub fn load(&self, ordering: atomic::Ordering) -> Guard<T> {}
    pub fn store(&self, new: Box<T>, ordering: atomic::Ordering) {}
    pub fn swap(&self, new: Box<T>, ordering: atomic::Ordering) -> Guard<T> {}
    pub fn compare_and_swap(&self, old: &T, new: Box<T>, ordering: atomic::Ordering)
    -> Result<Guard<T>, (Guard<T>, Box<T>)> {

    }
}
