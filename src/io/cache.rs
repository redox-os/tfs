use crossbeam::sync::SegQueue;

/// A writable guard to a cache block.
type WriteGuard<'a> = chashmap::WriteGuard<'a, disk::Sector, Block>;

/// The default initial capacity of the sector map.
const INITIAL_CAPACITY: usize = 256;

// TODO: Merge `Transaction` and `Transacting`.

/// A transaction handler
///
/// This is used for representing a write transaction node. It has a reference to the cache and a
/// mutable reference to the block, allowing it to create child transactions of the write.
///
/// The transaction will be flushable when this handler is dropped.
#[derive(Copy, Clone)]
#[must_use]
struct Transaction<'a> {
    /// The sector of the transaction.
    sector: disk::Sector,
    /// The block in question.
    ///
    /// This is a lock guard, and thus as long it is held, the block cannot be flushed.
    block: chashmap::WriteGuard<'a>,
}

impl<'a> Transaction<'a> {
    /// Wrap a value in the transaction.
    ///
    /// This makes a `Transacting` with the transaction and inner value `inner`.
    pub fn wrap<T>(self, inner: T) -> Transacting<T> {
        Transacting {
            inner: T,
            transaction: self,
        }
    }

    /// Execute a transaction depending on the current.
    ///
    /// This makes a new transaction which will execute `self` transaction then `other`.
    ///
    /// Beware that it will make `self` flushable, by executing the lock.
    pub fn then(self, other: Transaction) -> Transaction {
        // Make `other` depend on `self`.
        other.block.flush_dependencies.push(self.sector);

        // Since `other` depends on `self`, we can safely use `other`.
        Transaction {
            sector: other.sector,
            block: other.block,
        }

        // Now `self` will drop, releasing the lock, and making it flushable.
    }

    /// Execute the transaction.
    pub fn execute(self) {
        // Release the lock.
        drop(self.block);
        // To avoid the safety destructor from running, we leak `self`, knowing that everything is
        // deallocated.
        mem::forget(self);
    }
}

impl<'a> Drop for Transaction<'a> {
    fn drop(&mut self) {
        // In order to avoid accidentally dropping transactions, we force the programmer to
        // manually execute transactions.
        panic!("Dropping a `Transaction` automatically. Use `.execute()` instead.");
    }
}

/// A monad-like wrapper telling that some data depends on a transaction.
///
/// This wraps some inner data, adding a transaction, which it depends on.
///
/// It is usually used if some data requires a transaction to be valid. An example is if a file was
/// allocated on an address, then the address would only be valid if the associated transaction was
/// executed.
#[must_use]
struct Transacting<'a, T> {
    /// The inner data.
    inner: T,
    /// The accompanying transaction.
    transaction: Option<Transaction<'a>>,
}

impl<'a, T> Transacting<'a, T> {
    /// Create a new `Transacting`.
    ///
    /// This creates a new `Transacting<T>` from inner value `inner` and with transaction
    /// `Transaction`.
    fn new(inner: T, transaction: Option<Transaction>) -> Transacting<T> {
        Transacting {
            inner: inner,
            transaction: transaction,
        }
    }

    /// Create a new `Transacting` without a transaction.
    fn no_transaction(inner: T) -> Transacting<T> {
        Transacting {
            inner: T,
            transaction: None,
        }
    }

    /// Chain the transaction together with another transaction, so they're executed sequentially.
    ///
    /// This makes a new transaction which will execute `self`'s transaction then `other`.
    fn then(self, other: Transaction) -> Transaction {
        if let Some(transaction) = self.transaction {
            // Append it to the current transaction.
            transaction.then(other)
        } else {
            // `self` contained no transaction, so we'll simply execute `other`.
            other
        }
    }
}

/// A cache block.
///
/// This stores a single sector in memory, for more performant reads and writes.
///
/// The terminology "cache block" is similar to "cache lines" in CPUs. They represent a single
/// fixed-size block which is cached in memory.
struct Block {
    /// The data of the sector.
    ///
    /// This shall reflect what is on the disk, unless the block is marked "dirty".
    data: disk::SectorBuf,
    /// Is this block in sync with the disk?
    ///
    /// This is set if `self.data` matches what is written to the disk. Namely, if it has been
    /// flushed or not.
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
    /// Create a new block with some data.
    fn new(data: disk::SectorBuf) -> Block {
        Block {
            data: data,
            dirty: false,
            flush_dependencies: Vec::new(),
        }
    }
}

/// A cache operation.
enum CacheOperation {
    /// Create a new cache block for some sector.
    Create(disk::Sector),
    /// Touch a cache block of some sector.
    Touch(disk::Sector),
}

/// A cached disk.
///
/// This wrapper manages caching and the consistency issues originating from it.
///
/// It organizes the cache into _cache blocks_ each representing some _disk sector_. The cache
/// blocks are put in a _dependency graph_ which enforces the ordering of flushes (writes to
/// disks).
///
/// It holds a vdev driver, which the flushes are written to.
struct Cache {
    /// The inner driver.
    driver: vdev::Driver,

    /// The cache tracker operation queue.
    ///
    /// In order to avoid excessively locking and unlocking the cache tracker, we buffer the
    /// operations, which will then be executed in one go, when needed.
    queue: SegQueue<CacheOperation>,
    /// The cache replacement tracker.
    ///
    /// This tracks the state of the replacement algorithm, which chooses which cache block shall
    /// be replaced in favor of a new cache. It serves to estimate/guess which block is likely not
    /// used in the near future.
    tracker: Mutex<mlcr::Cache>,

    /// The sector-to-cache block map.
    sector_map: CHashMap<disk::Sector, Block>,
}

impl From<vdev::Driver> for Cache {
    fn from(driver: vdev::Driver) -> Cache {
        Cache {
            // Store the driver.
            driver: driver,
            // Set empty/default values.
            queue: SegQueue::new(),
            tracker: Mutex::new(mlcr::Cache::new()),
            sector_map: CHashMap::with_capacity(INITIAL_CAPACITY),
        }
    }
}

impl Cache {
    /// Execute a write transaction.
    ///
    /// This creates a transaction writing `buf` into sector `sector`, when dropped.
    fn write<F>(&self, sector: disk::Sector, buf: disk::SectorBuf) -> Transaction {
        debug!(self, "writing sector"; "sector" => sector);

        // Acquire the lock to the block, and initialize if it doesn't already exist.
        let lock = self.sector_map.get_mut_or(sector, Block::default());
        // Set the dirty flag.
        lock.dirty = true;
        // Update the data.
        lock.data = buf;

        Transaction {
            block: lock,
            cache: self,
        }
    }

    /// Read a sector.
    ///
    /// This reads sector `sector`, and applies the closure `map`. If `sector` needs to be fetched
    /// from the disk, and `map` fails, data recovery is attempted.
    ///
    /// If an I/O operation fails, the error is returned. Otherwise, the return value of `map` is
    /// returned.
    fn read_then<F, T, E>(&self, sector: disk::Sector, map: F) -> Result<T, E>
        where F: Fn(&disk::SectorBuf) -> Result<T, E>,
              E: From<disk::Error> {
        debug!(self, "reading sector"; "sector" => sector);

        // Check if the sector is already available in the cache.
        if let Some(accessor) = self.sector_map.find(sector) {
            // Yup, we found the sector in the cache.
            trace!(self, "cache hit; reading from cache"; "sector" => sector);

            // Touch the sector.
            self.queue.push(CacheOperation::Touch(sector));

            handler(accessor)
        } else {
            trace!(self, "cache miss; reading from disk"; "sector" => sector);

            // Occupy the block in the map, so that we can later on insert it.
            let block = self.sector_map.get_mut_or(sector, Block::default());
            // Insert the sector into the cache tracker.
            self.queue.push(CacheOperation::Create(sector));

            // Fetch the data from the disk.
            self.disk.read_to(sector, &mut block.data)?;

            // Handle the block through the closure, and resolve respective verification issues.
            match map(&block.data) {
                Err(err) => {
                    // The verifier failed, so the data is likely corrupt. We'll try to recover the
                    // data through the vdev's redundancy, then we read it again and see if it passes
                    // verification this time. If not, healing failed, and we cannot do anything about
                    // it.
                    warn!(self, "data verification failed"; "sector" => sector, "error" => err);

                    // Attempt to heal the sector.
                    self.disk.heal(sector)?;
                    // Read it again.
                    self.disk.read_to(sector, &mut block.data)?;

                    // Try to verify it once again.
                    map(&block.data)
                },
                x => x,
            }
        }
    }

    /// Trim the cache.
    ///
    /// This reduces the cache to exactly `to` blocks. Note that this is quite expensive, and
    /// should thus only be called once in a whle.
    fn trim(&self, to: usize) -> Result<(), disk::Error> {
        info!(self, "trimming cache"; "to" => to);

        // Lock the cache tracker.
        let tracker = self.tracker.lock();
        // Commit the buffered operations to the tracker.
        while let Some(op) = self.queue.try_pop() {
            match op {
                // Create new cache block.
                CacheOperation::Create(sector) => tracker.create(sector),
                // Touch a cache block.
                CacheOperation::Touch(sector) => tracker.touch(sector),
            }
        }

        // The set of blocks to trim.
        let mut flush: HashSet<_> = tracker.trim(to).collect();
        // Exhaust the set until it is empty.
        while let Some(tl_sector) = flush.drain().first() {
            debug!(self, "flushing and removing block"; "sector" => tl_sector);

            // Acquire the lock. To avoid changes while we traverse, we use mutable locks,
            // excluding other writes.
            // TODO: Perhaps use immutable locks and then upgrade them to mutable when changing the
            //       dirty flag. This could improve performance significantly.
            let tl_block = self.sector_map.get_mut(tl_sector);
            // Skip flushing, if the block is not dirty.
            if !tl_block.dirty {
                continue;
            }
            // Start with an empty stack to do our search. This stack will hold the state of the
            // traversal, following a variant of DFS, where we dive as deep as possible first, and
            // then backtrack when we can go deeper. Every element must be a dirty block.
            let mut stack = Vec::new();
            // Push the sector we will flush to the stack.
            stack.push((tl_sector, tl_block));

            // Traverse!
            loop {
                // Pop the top of the stack to deepen it.
                if let Some((sector, block)) = stack.pop() {
                    // See if the block has flush dependencies, which must be flushed before.
                    if let Some(dep) = block.flush_dependencies.pop() {
                        // It got at least one flush dependencies.
                        trace!(self, "traversing dependency";
                               "sector" => sector,
                               "depending sector" => dep);

                        // Since it could potentially have more than one dependencies, we push it
                        // back so that we can reinvestigate later.
                        stack.push((sector, block));

                        // Check if the block is dirty, or we can skip it. This holds up the
                        // invariant that every block in `stack` are dirty.
                        if block.dirty {
                            // Push the dependency to the stack.
                            stack.push((dep, self.sector_map.get_mut(sector)));
                        }
                    } else {
                        // The block is dirty and needs to be flushed. Note that we need not to
                        // check if it is dirty, as our invariant states that al blocks in `stack`
                        // are dirty.
                        debug!(self, "flushing block"; "sector" => sector);

                        // No more flush dependencies on the former top of the stack (`block`), so
                        // we can safely write the sector, knowing that all dependencies have been
                        // flushed.
                        self.driver.write(sector, block.data)?;
                        // Unset the dirty flag.
                        block.dirty = false;

                        // Clean up the block if it is a top-level block (a block which will be
                        // removed).
                        if flush.remove(sector) || sector == tl_sector {
                            // We've now removed the block from `flush`, so we won't flush it later
                            // on.

                            // Remove the sector from the cache tracker.
                            tracker.remove(sector);
                            // Finally, remove the block from the sector map.
                            self.sector_map.remove(sector);
                        }

                        // We don't need to pop from the stack, since we have already popped an
                        // element, which we won't push back. The lock originating from `get_mut`
                        // is dropped here, and the block can then freely be modified.
                    }
                } else {
                    // The stack is empty, and we've traversed everything.
                    break;
                }
            }
        }
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        info!(self, "closing cache");

        self.trim(0);
    }
}

delegate_log!(Cache.driver);

// TODO: Add tests.
