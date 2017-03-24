const POINTERS_IN_NODE: u64 = disk::SECTOR_SIZE / page::POINTER_SIZE;

struct Array<T> {
    root: page::Pointer,
    len: u64,
    _phantom: PhantomData<T>,
}

impl Array<T> {
    fn is_leaf(&self) -> bool {
        self.len <= POINTERS_IN_NODE
    }

    fn for_each<F>(&self, fs: &fs::State, range: Range, f: F) -> impl Future<(), alloc::Error>
    where F: Fn(usize, page::Pointer) {
        unimplemented!();
    }
}

impl<T: Object + From<page::Pointer>> Object {
    fn gc_visit(&self, fs: &fs::State) -> impl Future<(), alloc::Error> {
        unimplemented!();
    }
}
