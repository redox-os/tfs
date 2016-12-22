//! Page management.
//!
//! Pages are virtual data units of size 4088 bytes. They're represented on disk somewhat
//! non-obviously, since clusters can hold more than one page at once (compression). Every cluster
//! will maximize the number of pages held and when it's filled up, a new cluster will be fetched.

struct Metacluster {
    free: Vec<cluster::Pointer>,
}

impl Metacluster {
    fn checksum(&self) -> u32 {

    }
}

/// The page manager.
///
/// This is the center point of the I/O stack, providing allocation, deallocation, compression,
/// etc. It manages the clusters (with the page abstraction) and caches the disks.
struct Manager<D> {
    /// The cached disk header.
    ///
    /// The disk header contains various very basic information about the disk and how to interact
    /// with it.
    ///
    /// In reality, we could fetch this from the `disk` field as-we-go, but that hurts performance,
    /// so we cache it in memory.
    header: header::DiskHeader,
    /// The state block.
    ///
    /// The state block stores the state of the file system including allocation state,
    /// configuration, and more.
    state_bock: state_block::StateBlock,
    /// The inner disk.
    disk: Cache<D>,
    /// The head of the freelist.
    ///
    /// This list is used as the allocation primitive of TFS. It is a simple freelist-based extent
    /// allocation system, but there is one twist: To optimize the data locality, the list is
    /// unrolled.
    freelist_head: Metacluster,
}

impl<D: Disk> Manager<D> {
    fn freelist_pop(&mut self) -> cluster::Pointer {
        let cluster = self.freelist_head[self.freelist_head.counter];
        self.freelist_head.counter -= 1;
        self

        self.header.freelist_head
    }
}
