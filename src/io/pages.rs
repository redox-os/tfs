//! Page management.
//!
//! Pages are virtual data units of size 4088 bytes. They're represented on disk somewhat
//! non-obviously, since clusters can hold more than one page at once (compression). Every cluster
//! will maximize the number of pages held and when it's filled up, a new cluster will be fetched.

struct Metacluster {
    free: Vec<cluster::Pointer>,
}

enum Error {
    OutOfClusters,
    Disk(disk::Error),
}

/// A state of a page manager.
struct State {
    /// The state block.
    ///
    /// The state block stores the state of the file system including allocation state,
    /// configuration, and more.
    state_block: state_block::StateBlock,
    /// The head of the freelist.
    ///
    /// This list is used as the allocation primitive of TFS. It is a simple freelist-based extent
    /// allocation system, but there is one twist: To optimize the data locality, the list is
    /// unrolled.
    freelist_head: Metacluster,
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
    /// The inner disk.
    disk: Cache<D>,
    /// The state of the manager.
    state: State,
    /// The state of the manager on the time of last cache commit.
    ///
    /// This contains the state of the page manager upon the last cache commit (pipeline flush). It
    /// is used to roll back the page manager when an error occurs.
    committed_state: State,
}

impl<D: Disk> Manager<D> {
    fn commit(&mut self) {
        // Update the stored committed state to the current state, which we will commit.
        self.committed_state = self.state.clone();
        // Commit the cache pipeline.
        self.disk.commit();
    }

    fn revert(&mut self) {
        // Revert the state to when it was committed last time.
        self.state = self.committed_state.clone();
        // Revert the cache pipeline.
        self.disk.revert();
    }

    fn queue_freelist_pop(&mut self) -> Result<cluster::Pointer, Error> {
        // Pop from the metacluster.
        if let Some(cluster) = self.freelist_head.free.pop() {
            if self.freelist.head.free.is_empty() {
                // The head metacluster is exhausted, so we load the next metacluster (specified to be
                // the last pointer in the metacluster), i.e. `cluster`. The old metacluster is then
                // used as the popped cluster.
                mem::swap(&mut self.state.state_block.cluster, &mut cluster);
                self.state.freelist_head.update(self.disk.read(self.state.freelist_head.cluster)?);

                // We've updated the state block, so we queue a flush to the disk.
                self.queue_state_block_flush();
            } else {
                // Since the freelist head was changed after the pop, we queue a flush.
                self.queue_freelist_head_flush();
            }

            Ok(cluster)
        } else {
            // We ran out of clusters :(.
            Err(Error::OutOfClusters)
        }
    }

    fn queue_freelist_push(&mut self, cluster: cluster::Pointer) -> Result<(), Error> {
        if self.state.freelist_head.free.len() == cluster::SIZE / cluster::POINTER_SIZE {
            // The freelist head is full, and therefore we use following algorithm:
            //
            // 1. Create a new metacluster at `cluster`.
            // 2. Link said metacluster to the old metacluster.
            // 3. Queue a flush.

            // Clear the in-memory freelist head mirror.
            self.state.freelist_head.free.clear();
            // Put the link to the old freelist head into the new metacluster.
            self.state.freelist_head.push(state_block.freelist_head);

            // Update the freelist head pointer to point to the new metacluster.
            self.state.state_block.freelist_head = cluster;
            // Queue a flush of the new freelist head. This won't leave the system in an
            // inconsistent state as it merely creates a new metacluster, which is first linked
            // later. If the state block flush fails, the metacluster will merely be an orphan
            // cluster, and therefore simply leaked space.
            self.queue_freelist_head_flush();
            // Queue a flush of the state block (or, in particular, the freelist head pointer).
            // This is completely consistent as the freelist head must flush before, thus rendering
            // the pointed cluster a valid metacluster.
            self.queue_state_block_flush();
        } else {
            // There is space for more clusters in the head metacluster.

            // Push the cluster pointer to the freelist head.
            self.state.freelist_head.free.push(cluster);
            // Queue a flush of the new freelist head.
            self.queue_freelist_head_flush();
        }
    }
}
