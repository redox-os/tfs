const POINTERS_IN_NODE: u64 = disk::SECTOR_SIZE / page::POINTER_SIZE;

struct Array<T> {
    root: page::Pointer,
    len: u64,
    _phantom: PhantomData<T>,
}

impl Array<T> {
    fn depth(&self) -> u64 {
        64 - self.len.leading_zeros() as u64
    }

    fn capacity(&self) -> u64 {
        1 << self.depth()
    }

    fn is_leaf(&self) -> bool {
        self.len <= POINTERS_IN_NODE
    }

    fn full_child() -> u64 {
        self.capacity() / POINTERS_IN_NODE
    }

    fn is_full() -> bool {
        self.len == self.capacity()
    }

    fn get_ptr(&self, fs: &fs::State, n: u8) -> Result<page::Pointer, alloc::Error> {
        let buf = fs.read(root)?;
        Ok(little_endian::read(&buf[n * page::POINTER_SIZE..]))
    }

    fn get_child(&self, fs: &fs::State, n: u8) -> Result<Array<T>, alloc::Error> {
        let depth = self.depth();

        Array {
            root: self.get_ptr(fs, n),
            len: if n == POINTERS_IN_NODE - 1 {
                self.len - self.full_child() * POINTERS_IN_NODE
            } else {
                self.full_child()
            },
            _phatom: PhantomData,
        }
    }

    fn get_leaf(&self, fs: &fs::State, mut idx: u64) -> Result<Array<T>, alloc::Error> {
        let mut node = self;
        while !node.is_leaf() {
            idx = idx / node.full_child();
            node = node.get(fs, idx)?;
        }

        Ok(node)
    }

    fn get(&self, fs: &fs::State, idx: u64) -> Result<Option<page::Pointer>, alloc::Error> {
        if idx < self.len {
            if self.len == 1 {
                Ok(Some(self.ptr))
            } else {
                Ok(Some(self.get_leaf(fs, idx)?.get_ptr(idx % POINTERS_IN_NODE)?))
            }
        } else {
            Ok(None)
        }
    }

    fn write<I>(&self, fs: &fs::State, idx: u64, arr: I) -> Result<(), alloc::Error>
        where I: ExactSizeIterator + Iterator<Item = page::Pointer> {
        unimplemented!();
    }

    fn push(&mut self, fs: &fs::State, ptr: page::Pointer) -> Result<(), alloc::Error> {
        if self.len == 0 {
            self.root = ptr;
        } else if self.is_full() {
            let mut buf = disk::SectorBuf::default();
            little_endian::write(self.ptr, &mut buf);
            little_endian::write(&mut buf[page::POINTER_SIZE..], ptr);

            self.ptr = fs.alloc(buf, "array node")?;
        } else {
            let leaf = self.get_leaf(fs, idx)?;
            let buf = fs.read(leaf.ptr)?;

            little_endian::write(&mut buf[page::POINTER_SIZE * (idx % POINTERS_IN_NODE)..], ptr);
        }

        self.len += 1;
    }

    fn pop(&mut self)
}

impl<T: Object + From<page::Pointer>> Object {
    fn gc_visit(&self, fs: &fs::State) -> Result<(), alloc::Error> {
        // TODO: This recursion can be very deep and potentially lead to stack overflow.
        // TODO: This should run in parallel.

        // We make sure that the tree isn't empty. If it is, we have nothing to traverse.
        if self.len != 0 {
            if self.len == 1 {
                // This node is a leaf, so we cast the pointer into an object and traverse it.
                fs.visit(T::from(self.root))?;
            } else {
                // Set the root as reachable.
                fs.set_reachable(self.root)?;
                // Go over the pointers in the root and visit the children.
                for i in 0..self.len / self.full_child() {
                    fs.visit(self.get_child(i)?);
                }
            }
        }
    }
}
