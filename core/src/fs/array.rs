use futures::Future;
use std::marker::PhantomData;
use std::ops::Range;

use {disk, fs, Error};
use alloc::page;

const POINTERS_IN_NODE: u64 = disk::SECTOR_SIZE / page::POINTER_SIZE;

struct Array<T> {
    root: page::Pointer,
    len: u64,
    _phantom: PhantomData<T>,
}

impl<T> Array<T> {
    fn is_leaf(&self) -> bool {
        self.len <= POINTERS_IN_NODE
    }

    fn for_each<F>(&self, fs: &fs::State, range: Range<u64>, f: F) -> future!(())
    where F: Fn(usize, page::Pointer) {
        unimplemented!();
    }
}

impl<T: fs::Object + From<page::Pointer>> fs::Object for Array<T> {
    fn gc_visit(&self, fs: &fs::State) -> future!(()) {
        unimplemented!();
    }
}
