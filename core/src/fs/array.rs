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

    fn for_each<F>(&self, fs: &fs::State, range: Range, f: F) -> Result<(), alloc::Error>
        where F: Fn(usize, page::Pointer) -> Result<(), alloc::Error> {
        self.for_each_idx(fs, range.start, range, f)
    }

    fn for_each_idx<F>(&self, fs: &fs::State, idx: usize, range: Range, f: F) -> Result<(), alloc::Error>
        where F: Fn(usize, page::Pointer) -> Result<(), alloc::Error> {
        if self.is_leaf() {
            let buf = fs.read(self.root)?;
            for i in range {
                f(idx + i, little_endian::read(&buf[i * page::POINTER_SIZE..]))?;
            }
        } else {
            let max_child = (self.len + POINTERS_IN_NODE / 2) / POINTERS_IN_NODE;

            self.for_each_idx(idx, range.start..cmp::min(range.end, range.start + POINTERS_IN_NODE), f)?;

            for i in (POINTERS_IN_NODE..range.end).step_by(max_child) {
                self.for_each_idx()
            }
        }
    }
}

impl<T: Object + From<page::Pointer>> Object {
    fn gc_visit(&self, fs: &fs::State) -> Result<(), alloc::Error> {
    }
}
