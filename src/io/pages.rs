//! Page management.
//!
//! Pages are virtual data units of size 4088 bytes. They're represented on disk somewhat
//! non-obviously, since clusters can hold more than one page at once (compression). Every cluster
//! will maximize the number of pages held and when it's filled up, a new cluster will be fetched.

/// The size (in bytes) of the general cluster header, shared by data clusters and metaclusters.
const GENERAL_CLUSTER_HEADER: usize = 2;
/// The size (in bytes) of the metacluster header.
///
/// This includes the general cluster header.
const METACLUSTER_HEADER: usize = GENERAL_CLUSTER_HEADER + 2;

/// A page management error.
enum Error {
    /// No clusters left in the freelist.
    ///
    /// This is the equivalent to OOM, but with disk space.
    OutOfClusters,
    /// The checksum of the data and the provided checksum does not match.
    ///
    /// This indicates some form of data corruption.
    ChecksumMismatch,
    /// The compressed data is invalid and cannot be decompressed.
    ///
    /// Multiple reasons exists for this to happen:
    ///
    /// 1. The compression configuration option has been changed without recompressing clusters.
    /// 2. Silent data corruption occured, and did the unlikely thing to has the right checksum.
    /// 3. There is a bug in compression or decompression.
    InvalidCompression,
    /// A disk error.
    Disk(disk::Error),
}

/// A state of a page manager.
struct State {
    /// The state block.
    ///
    /// The state block stores the state of the file system including allocation state,
    /// configuration, and more.
    state_block: state_block::StateBlock,
    /// The first chunk of the freelist.
    ///
    /// This list is used as the allocation primitive of TFS. It is a simple freelist-based extent
    /// allocation system, but there is one twist: To optimize the data locality, the list is
    /// unrolled.
    ///
    /// The first element (if any) points to _another_ freelist chunk (a "metacluster"), which can
    /// be used to traverse to the next metacluster when needed.
    freelist: Vec<cluster::Pointer>,
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
    /// Commit the transactions in the pipeline to the cache.
    ///
    /// This runs over the transactions in the pipeline and applies them to the cache. In a sense,
    /// it can be seen as a form of checkpoint as you can revert to the last commit through
    /// `.revert()`, as it stores the old state.
    fn commit(&mut self) {
        // Update the stored committed state to the current state, which we will commit.
        self.committed_state = self.state.clone();
        // Commit the cache pipeline.
        self.disk.commit();
    }

    /// Revert to the last commit.
    ///
    /// This will reset the state to after the previous cache commit.
    fn revert(&mut self) {
        // Revert the state to when it was committed last time.
        self.state = self.committed_state.clone();
        // Revert the cache pipeline.
        self.disk.revert();
    }

    /// Calculate the checksum of some buffer, based on the user configuration.
    fn checksum(&self, buf: &[u8]) -> u16 {
        // The behavior depends on the chosen checksum algorithm.
        match self.state.state_block.checksum {
            // Constant checksums. These are a bit weird, but in the end it makes sense: a number
            // is fixed to some value (in this case, the highest 16-bit integer), under the
            // assumption that if a sector is damaged, all of it is affected, hence the number
            // shouldn't match. Obviously, this isn't true for every disk, therefore one must
            // be careful before using this.
            ChecksumAlgorithm::Constant => 0xFFFF,
            // Hash the thing via SeaHash, then take the 16 lowest bits (truncating cast).
            ChecksumAlgorithm::SeaHash => seahash::hash(buf) as u16,
        }
    }

    /// Compress some data based on the compression configuration option.
    ///
    /// This compresses `source` into `target` based on the chosen configuration method, defined in
    /// the state block.
    fn compress(&self, source: &[u8], target: &mut Vec<u8>) {
        match self.state.state_block.compression_algorithm {
            // Memcpy as a compression algorithm!!!11!
            CompressionAlgorithm::Identity => target.extend_from_slice(source),
            // Compress via LZ4.
            CompressionAlgorithm::Lz4 => lz4_compress::compress_into(source, target),
        }
    }

    /// Decompress some data based on the compression configuration option.
    ///
    /// This decompresses `source` into `target` based on the chosen configuration method, defined
    /// in the state block.
    fn decompress(&self, source: &[u8], target: &mut Vec<u8>) -> Result<(), Error> {
        match self.state.state_block.compression_algorithm {
            // Memcpy as a compression algorithm!!!11!
            CompressionAlgorithm::Identity => target.extend_from_slice(source),
            // Decompress from LZ4.
            CompressionAlgorithm::Lz4 => lz4_compress::decompress_from(source, target)?,
        }

        Ok(())
    }

    /// Queue a state block flush.
    ///
    /// This queues a new transaction flushing the state block.
    fn queue_state_block_flush(&mut self) {
        self.disk.queue(self.header.state_block_address, self.state.state_block.into());
    }

    /// Queue a freelist head flush.
    ///
    /// This queues a new transaction flushing the freelist head.
    fn queue_freelist_head_flush(&mut self) {
        // Start with an all-null cluster buffer.
        let mut buf = Box::new([0; disk::SECTOR_SIZE]);

        // Write every pointer of the freelist into the buffer.
        for (n, i) in self.free.iter().enumerate() {
            LittleEndian::write(&mut buf[cluster::POINTER_SIZE * i + METACLUSTER_HEADER..], i);
        }

        // Checksum the non-checksum part of the buffer, and write it at the start of the buffer.
        LittleEndian::write(&mut buf, self.checksum(&buf[2..]));

        // Queue the write of the updated buffer.
        self.disk.queue(self.state.state_block.freelist_head, buf);
    }

    /// Queue a pop from the freelist.
    ///
    /// This adds a new transaction to the cache pipeline, which will pop from the top of the
    /// freelist and return the result.
    fn queue_freelist_pop(&mut self) -> Result<cluster::Pointer, Error> {
        // Pop from the metacluster.
        if let Some(cluster) = self.state.freelist.pop() {
            if self.freelist.head.free.is_empty() {
                // The head metacluster is exhausted, so we load the next metacluster (specified to be
                // the last pointer in the metacluster), i.e. `cluster`. The old metacluster is then
                // used as the popped cluster.
                mem::swap(&mut self.state.state_block.cluster, &mut cluster);
                self.state.load_freelist(self.disk.read(self.state_block.freelist_head)?);

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

    /// Queue a push to the freelist.
    ///
    /// This adds a new transaction to the cache pipeline, which will push some free cluster to the
    /// top of the freelist.
    fn queue_freelist_push(&mut self, cluster: cluster::Pointer) -> Result<(), Error> {
        if self.state.freelist.len() == cluster::SIZE / cluster::POINTER_SIZE {
            // The freelist head is full, and therefore we use following algorithm:
            //
            // 1. Create a new metacluster at `cluster`.
            // 2. Link said metacluster to the old metacluster.
            // 3. Queue a flush.

            // Clear the in-memory freelist head mirror.
            self.state.freelist.clear();
            // Put the link to the old freelist head into the new metacluster.
            self.state.freelist.push(state_block.freelist_head);

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
            self.state.freelist.push(cluster);
            // Queue a flush of the new freelist head.
            self.queue_freelist_head_flush();

            // lulz @ these comments. like shit, ticki, they add basically nothing you fuking dumb
            // monkey. seriously stop it
        }
    }
}
