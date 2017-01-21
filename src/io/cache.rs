/// A cache block.
///
/// This stores a single sector in memory, for more performant reads and writes.
///
/// The terminology "cache block" is similar to "cache lines" in CPUs. They represent a single
/// fixed-size block which is cached in memory.
struct Block {
    /// The sector the block stores.
    sector: disk::Sector,
    /// The data of the sector.
    ///
    /// This shall reflect what is on the disk unless the block is marked dirty.
    ///
    /// We do not need to store this on the heap, but we do in order to improve performance and
    /// avoid excessive copying between stack frames.
    data: Box<disk::SectorBuf>,
    /// Does the data in memory reflect the data on the disk?
    ///
    /// This is called _the dirty flag_ and defines if a flush is needed or if the in-memory data
    /// already matches the on-disk data. Whenever it is written in memory, the flag should be set
    /// so that we're sure that it gets flushed properly to the disk.
    dirty: bool,
    /// Disk sectors that shall be flushed _before_ this block.
    ///
    /// This defines the flush dependencies and is crucial to the consistency of the cache. In
    /// particular, ordering matters, since unexpected crashes should never leave the system in an
    /// inconsistent state.
    ///
    /// To consider flush ordering, we define a poset on the cache blocks. If `A < B`, `A` should
    /// be written prior to `B`. This is represented as an ADG. The algorithm for flushing is:
    /// write the orphan nodes to the disk, until no nodes are dirty.
    ///
    /// In other words, the sectors in this vector are _guaranteed_ to be written before the block
    /// itself.
    flush_dependencies: Vec<disk::Sector>,
}

impl Block {
    /// Reset the cache block.
    ///
    /// This resets the dirty flag and clears the flush dependencies.
    ///
    /// Note that it does not change the data or the sector.
    fn reset(&mut self) {
        // The cache block starts out as clean...
        self.dirty = false;
        // ...and hence has no dependencies.
        self.flush_dependencies.clear();
    }

    /// Add a sector to flush before the block.
    fn add_dependency(&mut self, sector: disk::Sector) {
        // To avoid meta-cycles, we make sure that the dependent sector isn't the sector of the
        // block itself.
        if self.sector != sector {
            self.flush_dependencies.push(sector);
        }
    }
}

/// A cached disk.
///
/// This wrapper manages caching and the consistency issues originating from it.
///
/// It organizes the cache into _cache blocks_ each representing some _disk sector_. The cache
/// blocks are put in a _dependency graph_ which enforces the ordering of flushes (writes to
/// disks).
struct Cache<D> {
    /// The raw disk.
    disk: D,
    /// The cache replacement tracker.
    ///
    /// This tracks the state of the replacement algorithm, which chooses which cache block shall
    /// be replaced in favor of a new cache. It serves to estimate/guess which block is likely not
    /// used in the near future.
    cache_tracker: mlcr::Cache,
    /// The cache blocks.
    blocks: HashMap<disk::Sector, Block>,
    /// The pipeline of writes to-be-committed.
    ///
    /// These are not committed to the block map yet and will not be until `.commit()` is called.
    /// They are ensured to be written to the disk in the order of the pipeline.
    pipeline: Vec<(disk::Sector, Box<disk::SectorBuf>)>,
}

impl<D: Disk> Cached<D> {
    /// Flush a sector to the disk.
    ///
    /// This can potentially trigger outer flushes if the cache block has flush dependencies.
    ///
    /// Note that this doesn't commit the pipeline.
    pub fn flush(&mut self, sector: disk::Sector) -> Result<(), disk::Error> {
        // Read the block.
        let block = &mut self.blocks[block];

        // Flush all the dependencies. This is important for correct ordering!
        for dep in block.flush_dependencies {
            self.flush(dep)?;

            // It could happen naturally that the dependent sector was not found in the block
            // map. Namely, if the sector was replaced by another cache block. In such case, the
            // sector is naturally already flushed (during replacement) and thus there is no
            // consistency issues.
        }

        // Check if the block is (still) dirty.
        if block.dirty {
            // Write the block to the disk.
            self.disk.write(block.sector, &block.data)?;
            // Unset the dirty flag.
            block.dirty = false;
        }
    }

    /// Flush all sectors to the disk.
    pub fn flush_all(&mut self) -> Result<(), disk::Error> {
        // Run over the block map and flush them.
        for i in self.blocks.keys() {
            self.flush(i);
        }
    }

    /// Read a sector from the disk.
    ///
    /// Note that this does not respond to writes in the pipeline, only committed transactions.
    pub fn read(&self, sector: disk::Sector) -> Result<&disk::SectorBuf, disk::Error> {
        Ok(self.get(sector)?.data)
    }

    /// Queue a write to the pipeline.
    ///
    /// This pushes a transaction to the pipeline, which can be committed through `.commit()`.
    #[inline]
    pub fn queue(&mut self, sector: disk::Sector, buf: disk::SectorBuf) {
        // For now, we'll just assume the function gets inlined such that copying the buffer from a
        // higher stack frame isn't needed.
        // TODO: Consider letting the call side allocate the buffer.
        self.pipeline.push((sector, box buf));
    }

    /// Commit the transactions in the pipeline to the cache.
    ///
    /// This commits the sectors and data given in the pipeline in the specified order enforcing
    /// consistency with respect to the flush order. To understand what this means, one must see
    /// writes as a function from a valid state to another together with the constraint that
    /// another function is applied prior to that. In other words, it does not enforce that they're
    /// written sequentially — or even written at all. Transactions can be forwarded backwards and
    /// merged with other transactions, but this should not leave the system inconsistent, since
    /// the _ordering_ is still enforced.
    ///
    /// More formally, we can think of the pipeline as a totally ordered set. When it is committed,
    /// every transaction is put into totally ordered set of transactions, such that the
    /// transactions preserving their order in the pipeline. If two transactions "collide" (are
    /// writing to the same sector), the newest one is picked and the old one is thrown away.
    pub fn commit(&mut self) {
        if Some((first_sector, first_buf)) = writes.next() {
            // Write the first block which has no dependencies.
            let mut block = self.commit_write(first_sector, first_buf, None);

            // Write the rest with the previous write as dependency.
            for (sector, buf) in self.pipeline.drain() {
                block = self.commit_write(sector, buf, Some(block.sector));
            }
        }
    }

    /// Commits a sector write with some dependency.
    ///
    /// This writes `buf` into sector `sector` in the cache, ensuring that the sector (if any)
    /// `dependency` is flushed to the disk prior to `sector`.
    fn commit_write(&mut self, sector: disk::Sector, buf: Box<disk::SectorBuf>, dependency: Option<disk::Sector>) -> &mut Block {
        // Allocate a new cache block.
        let block = cache.alloc_block(sector);

        // Put the data into the freshly allocated cache block.
        block.data = buf;

        // Add the potential dependency to the cache block.
        if let Some(dependency) = dependency {
            block.add_dependency(dependency)
        }
        // Mark dirty.
        block.dirty = true;

        block
    }

    /// Trim the cache to reduce memory.
    ///
    /// This reduces the cache to some fixed number of cache blocks, if the number of blocks is
    /// above some fixed limit.
    pub fn trim(&mut self) -> Result<(), disk::Error> {
        /// The maximum number of blocks before a trim will occur.
        const MAX_BLOCKS: usize = 500000;
        /// The minimum number of blocks after a trim has occured.
        ///
        /// If the number of cache blocks
        const MIN_BLOCKS: usize = 300000;

        // Make sure that there are enough blocks before trimming.
        if self.blocks.len() > MAX_BLOCKS {
            // Find candidates for trimming and remove them.
            for sector in self.cache_tracker.trim(MIN_BLOCKS) {
                self.remove(sector)?;
            }
        }

        Ok(())
    }

    /// Remove some sector from the trash.
    fn remove(&mut self, sector: disk::Sector) -> Result<(), disk::Error> {
        self.flush(block)?;
        self.blocks.remove(sector);

        Ok(())
    }

    /// Allocate (or find replacement) for a new cache block.
    ///
    /// This finds a cache block which can be used for new objects.
    ///
    /// It will reset and flush the block and update the block map.
    fn alloc_block(&mut self, sector: disk::Sector) -> &mut Block {
        // Note that we simply insert letting the cache grow. We will incidentally "trim" the cache
        // to reduce memory usage.
        self.blocks.insert(sector, Block {
            data: vec![0; disk::SECTOR_SIZE],
            dirty: false,
            flush_dependencies: Vec::new(),
        });

        // I wish there was a method to bypass this lookup, but there isn't, so we simply index.
        &mut self.blocks[sector]
    }

    /// Fetch an uncached disk sector to the cache.
    ///
    /// This will fetch `sector` from the disk to store it in the in-memory cache structure.
    fn fetch_fresh(&mut self, sector: disk::Sector) -> Result<&mut Block, disk::Error> {
        // Allocate a new cache block.
        let block = self.alloc_block(sector);

        // Read the sector from the disk.
        self.disk.read(sector, &mut block.data)?;

        // Add the cache block to the cache tracker.
        self.cache_tracker.insert(sector);
    }

    /// Get the cache block for a sector.
    ///
    /// This grabs the sector from the cache or from the disk, if necessary.
    fn get(&mut self, sector: disk::Sector) -> Result<&mut Block, disk::Error> {
        // Check if the sector already exists in the cache.
        if let Some(block) = self.blocks.get_mut(sector) {
            // It did!

            // Touch the cache block.
            self.cache_tracker.touch(sector);

            // Read the block.
            Ok(&mut self.blocks[block])
        } else {
            // It didn't, so we read it from the disk:
            self.fetch_fresh(sector)
        }
    }
}

impl<D: Disk> Drop for Cached<D> {
    fn drop(&mut self) {
        self.flush_all();
    }
}

// TODO: Add tests.
