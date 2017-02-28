struct State {
    alloc: alloc::Manager,
    reachable: cbloom::Filter,
}

impl Fs {
    pub fn alloc(&self, buf: disk::SectorBuf, description: &'static str) -> impl Future<page::Pointer, Error> {
        debug!(self, "allocating buffer", "description" => description);

        // Allocate the buffer.
        let ptr = self.alloc.alloc(buf).map(|ptr| self.visit(ptr))
        // Insert it into the set of currently reachable pages in case that it is reachable right
        // now.

        Ok(ptr)
    }

    pub fn set_reachable(&self, ptr: page::Pointer) {
        self.reachable.insert(ptr);
    }

    pub fn visit<T: Object>(&self, obj: T) -> Result<(), alloc::Error> {
        trace!(self, "visting object", "type" => type_name::get::<T>());

        obj.gc_visit(self)
    }
}

delegate_log!(State.alloc);
