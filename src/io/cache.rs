/// A cache block number.
///
/// Every cache block is assigned a number which is associated with the block in memory.
type BlockNumber = usize;

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
    data: Box<[u8]>,
    /// Does the data in memory reflect the data on the disk?
    ///
    /// This is called _the dirty flag_ and defines if a flush is needed or if the in-memory data
    /// already matches the on-disk data. Whenever it is written in memory, the flag should be set
    /// so that we're sure that it gets flushed properly to the disk.
    dirty: bool,
    /// Cache blocks that should be flushed _before_ this block.
    ///
    /// This defines the flush dependencies and is crucial to the consistency of the cache. In
    /// particular, ordering matters, since unexpected crashes should never leave the system in an
    /// inconsistent state.
    ///
    /// To consider flush ordering, we define a poset on the cache blocks. If `A < B`, `A` should
    /// be written prior to `B`. This is represented as an ADG. The algorithm for flushing is:
    /// write the orphan nodes to the disk, until no nodes are dirty.
    ///
    /// In other words, the blocks in this vector are _guaranteed_ to be written before the block
    /// itself.
    flush_dependencies: Vec<BlockNumber>,
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
}

/// A write transaction.
///
/// This is a transaction to commit some new data to a sector, either in memory or on disk.
struct WriteTransaction<'a> {
    /// The sector to write.
    sector: disk::Sector,
    /// The data to write.
    ///
    /// This buffer must be of the sector size.
    data: &'a [u8],
}

/// A cached disk.
///
/// This wrapper manages caching and the consistency issues originating from it.
///
/// It organizes the cache into _cache blocks_ each representing some _disk sector_. The cache
/// blocks are put in a _dependency graph_ which enforces the ordering of flushes (writes to
/// disks).
struct Cached<D> {
    /// The raw disk.
    disk: D,
    /// The disk sector size.
    sector_size: usize,
    /// The cache replacement tracker.
    ///
    /// This tracks the state of the replacement algorithm, which chooses which cache block shall
    /// be replaced in favor of a new cache. It serves to estimate/guess which block is likely not
    /// used in the near future. This guessed block is then replaced by a fresh new cache block.
    cache_tracker: plru::DynamicCache,
    /// The sector-to-cache-block map.
    ///
    /// This associates the disk sector with its respective cache block.
    block_map: RwLock<HashMap<disk::Sector, BlockNumber>>,
    /// The cache blocks.
    blocks: RwLock<Vec<[RwLock<Block>]>>,
}

impl<D: Disk> Cached<D> {
    /// Get the disk sector size.
    fn sector_size(&self) {
        self.sector_size
    }

    /// Allocate (or find replacement) for a new cache block.
    ///
    /// This finds a cache block which can be used for new objects.
    ///
    /// It will reset and flush the block and update the block map.
    fn alloc_block(&mut self) -> BlockNumber {
        // Test if the cache is filled.
        if self.blocks.len() < self.cache_tracker.len() {
            // The cache is not filled, so we don't need to replace any cache block, we can simply
            // push.
            self.blocks.push(Block {
                sector: sector,
                data: vec![0; self.disk.sector_size],
                dirty: false,
                flush_dependencies: Vec::new(),
            });

            self.blocks.len() - 1
        } else {
            // Find a candidate for replacement.
            let block_number = self.cache_tracker.replace();

            // Flush it to the disk before throwing the data away.
            self.flush(block_number);

            // Remove the old sector from the cache block map.
            let block = &mut self.blocks[block_number];
            self.block_map.remove(block.sector);

            // Reset the cache block.
            block.reset();

            block_number
        }
    }

    /// Fetch an uncached disk sector to the cache.
    ///
    /// This will fetch `sector` from the disk to store it in the in-memory cache structure.
    fn fetch_fresh(&mut self, sector: disk::Sector) -> Result<&mut Block, disk::Error> {
        // Allocate a new cache block.
        let block_number = self.alloc_block();
        let block = &mut self.blocks[block_number];

        // Read the sector from the disk.
        self.disk.read(sector, &mut block.data)?;

        // Update the sector number.
        block.sector = sector;

        // Update the cache block map with the new block.
        self.block_map.insert(sector, block_number);
    }

    /// Write a sector.
    ///
    /// This commits transaction `write` to the cache structure.
    ///
    /// There is no ordering guarantees.
    fn write(&mut self, write: WriteTransaction) -> BlockNumber {
        // Allocate a new cache block.
        let block_number = self.alloc_block();
        let block = &mut self.blocks[block_number];

        // Copy the data into the freshly allocated cache block.
        block.data.copy_from_slice(data);

        // Update the sector number.
        block.sector = sector;
        // Mark dirty.
        block.dirty = true;

        // Update the cache block map with the new block.
        self.block_map.insert(sector, block_number);

        block
    }

    /// Write a number of transactions in a fixed sequence.
    ///
    /// This has strict ordering guarantees, such that the first element of the iterator is
    /// written firstly, and the rest follows sequentially.
    fn write_seq<I: Iterator<Item = WriteTransaction>>(&mut self, writes: I) {
        // Execute the first query and store the number of the cache block in which it is written.
        let mut prev_block_number = self.write(writes.next().unwrap());
        // Execute the rest of the queries.
        for query in writes {
            // Execute the query.
            let block_number = self.write(query);
            // Push the previous query as a dependency on the write.
            self.blocks[block_number].flush_dependencies.push(prev_block_number);

            // Update the previous block number.
            prev_block_number = block_number;
        }
    }

    /// Get the cache block for a sector.
    ///
    /// This grabs the sector from the cache or from the disk, if necessary.
    fn get(&mut self, sector: disk::Sector) -> Result<&mut Block, disk::Error> {
        // Check if the sector already exists in the cache.
        if let Some(block) = self.block_map.get_mut(sector) {
            // It did!

            // Touch the cache block.
            self.cache_tracker.touch(block);

            // Read the block.
            Ok(&mut self.blocks[block])
        } else {
            // It didn't, so we read it from the disk:
            self.fetch_fresh(sector)
        }
    }

    /// Flush a cache block to the disk.
    ///
    /// This can potentially trigger outer flushes if the cache block has flush dependencies.
    fn flush(&mut self, block: BlockNumber) -> Result<(), disk::Error> {
        // Read the block.
        let block = &mut self.blocks[block];

        // Flush all the dependencies. This is important for correct ordering!
        for i in block.flush_dependencies {
            self.flush(i)?;
        }

        // Check if the block is (still) dirty.
        if block.dirty {
            // Write the block to the disk.
            self.disk.write(block.sector, &block.data)?;
            // Unset the dirty flag.
            block.dirty = false;
        }
    }
}
