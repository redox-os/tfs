mod array;
mod object;

pub use self::object::Object;

use {type_name, cbloom, alloc, Error};
use alloc::page;
use futures::Future;
use disk::{self, Disk};

struct State<D> {
    alloc: alloc::Allocator<D>,
    reachable: cbloom::Filter,
}

impl<D: Disk> State<D> {
    pub fn alloc(
        &self,
        buf: disk::SectorBuf,
        description: &'static str,
    ) -> future!(page::Pointer) {
        debug!(self, "allocating buffer"; "description" => description);

        // Allocate the buffer and insert it into the set of currently reachable pages in case that
        // it is reachable right now.
        Ok(self.alloc.alloc(buf).map(|ptr| self.visit(ptr)))
    }

    pub fn set_reachable(&self, ptr: page::Pointer) {
        self.reachable.insert(ptr);
    }

    pub fn visit<T: Object>(&self, obj: T) -> Result<(), Error> {
        trace!(self, "visting object"; "type" => type_name::get::<T>());

        obj.gc_visit(self)
    }
}

delegate_log!(State.alloc);
