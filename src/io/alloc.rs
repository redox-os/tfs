//! Page management.
//!
//! Pages are virtual data units of size 4088 bytes. They're represented on disk somewhat
//! non-obviously, since clusters can hold more than one page at once (compression). Every cluster
//! will maximize the number of pages held and when it's filled up, a new cluster will be fetched.

/// The atomic ordering used in the allocator.
const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;

quick_error! {
    /// A page management error.
    enum Error {
        /// No clusters left in the freelist.
        ///
        /// This is the equivalent to OOM, but with disk space.
        OutOfClusters {
            description("Out of free clusters.")
        }
        /// A page checksum did not match.
        ///
        /// The checksum of the data and the checksum stored in the page pointer did not match.
        ///
        /// This indicates some form of data corruption in the sector storing the page.
        PageChecksumMismatch {
            /// The page with the mismatching checksum.
            page: page::Pointer,
            /// The actual checksum of the page.
            found: u32,
        } {
            display("Mismatching checksums in {} - expected {:x}, found {:x}.",
                    page, page.checksum, found)
            description("Mismatching checksum in page.")
        }
        /// A metacluster checksum did not match.
        ///
        /// The checksum of the metacluster and the checksum stored in the previous metacluster
        /// pointer did not match.
        ///
        /// This indicates some form of data corruption in the sector storing the metacluster.
        MetacluterChecksumMismatch {
            /// The corrupted metacluster whose stored and actual checksum mismatches.
            cluster: cluster::Pointer,
            /// The expected/stored checksum.
            expected: u64,
            /// The actual checksum of the data.
            found: u64,
        } {
            display("Mismatching checksums in metacluster {:x} - expected {:x}, found {:x}.",
                    cluster, expected.checksum, found)
            description("Mismatching checksum in metacluster.")
        }
        /// The compressed data is invalid and cannot be decompressed.
        ///
        /// Multiple reasons exists for this to happen:
        ///
        /// 1. The compression configuration option has been changed without recompressing clusters.
        /// 2. Silent data corruption occured, and did the unlikely thing to has the right checksum.
        /// 3. There is a bug in compression or decompression.
        InvalidCompression {
            cluster: cluster::Pointer,
        } {
            display("Unable to decompress data from cluster {}.", cluster)
            description("Unable to decompress data.")
        }
        /// A disk error.
        Disk(err: disk::Error) {
            from()
            description("Disk I/O error.")
            display("Disk I/O error: {}", err)
        }
    }
}

/// The state of some cluster.
///
/// This caches a cluster uncompressed such that there is no need for decompression when appending
/// a new page into the cluster.
struct ClusterState {
    /// The pointer to the cluster.
    cluster: cluster::Pointer,
    /// The cluster uncompressed.
    ///
    /// This is used for packing pages into the cluster, by appending the new page to this vector
    /// and then compressing it to see if it fits into the cluster. If it fails to fit, the vector
    /// is reset and a new cluster is allocated.
    uncompressed: Vec<u8>,
}

/// The page manager.
///
/// This is the center point of the I/O stack, providing allocation, deallocation, compression,
/// etc. It manages the clusters (with the page abstraction) and caches the disks.
struct Manager {
    /// The inner disk cache.
    cache: Cache,
    /// The on-disk state.
    ///
    /// This is the state as stored in the state block. The reason we do not store the whole state
    /// block in one is that, we want to avoid the lock when reading the static parts of the state
    /// block (e.g. configuration).
    state: Mutex<state_block::State>,
    /// The configuration options.
    ///
    /// This is the configuration part of the state block. We don't need a lock, since we won't
    /// mutate it while the system is initialized.
    config: state_block::Config,
    /// The free-cache.
    ///
    /// This contains some number of pointers to free clusters, allowing multiple threads to
    /// efficiently allocate simultaneously.
    free: SegQueue<cluster::Pointer>,
    /// The last allocated cluster for this thread.
    ///
    /// If possible, newly allocated pages will be appended to this cluster. When it is filled
    /// (i.e. the pages cannot compress to the cluster size or less), a new cluster will be
    /// allocated.
    last_cluster: thread_object::Object<Option<ClusterState>>,
    /// The deduplication table.
    ///
    /// This table allows the allocator for searching for candidates to use instead of allocating a
    /// new cluster. In particular, it searches for duplicates of the allocated page.
    dedup_table: dedup::Table,
}

impl Manager {
    /// Open the manager from some driver.
    ///
    /// This loads the state page and other things from a vdev driver `driver`. If it fails, an
    /// error is returned.
    fn open(driver: vdev::Driver) -> Result<Manager, Error> {
        unimplemented!();
    }

    /// Allocate a page.
    ///
    /// This allocates a page with content `buf`.
    ///
    /// The algorithm works greedily by fitting as many pages as possible into the most recently
    /// used cluster.
    pub fn alloc(&mut self, buf: disk::SectorBuf) -> Result<cache::Transacting<page::Pointer>, Error> {
        // TODO: The variables are named things like `ptr`, which kinda contradicts the style of
        //       the rest of the code.

        /// The capacity (in bytes) of a compressed cluster.
        ///
        /// This is the maximal number of bytes that a cluster can contain decompressed.
        const CLUSTER_CAPACITY: usize = 512 * 2048;

        // Calculate the checksum of the buffer. We'll use this later.
        let cksum = self.checksum(buf) as u32;
        debug!(self, "allocating page"; "checksum" => cksum);

        // Check if duplicate exists.
        if let Some(page) = self.dedup_table.dedup(buf, cksum) {
            debug!(self, "found duplicate page"; "page" => page);
            // Deduplicate and simply use the already stored page. No transaction where required.
            return Ok(cache::Transacting::no_transaction(page));
        }

        // Handle the case where compression is disabled.
        if self.config.compression_algorithm == CompressionAlgorithm::Identity {
            // Pop a cluster from the freelist.
            let cluster = self.freelist_pop()?;

            let ptr = page::Pointer {
                cluster: cluster,
                offset: None,
                checksum: cksum,
            };

            // Insert the page pointer into the deduplication table to allow future use as
            // duplicate.
            self.dedup_table.insert(buf, ptr);

            // Write the cluster with the raw, uncompressed data, and return the transaction monad.
            return Ok(cluster.then(self.cache.write(cluster, buf)).wrap(ptr));
        }

        self.last_cluster.with(|x| if let Some(state) = x {
            // We have earlier allocated a cluster, meaning that we can potentially append more
            // pages into the cluster.

            // Check if the capacity of the cluster is exceeded. If so, jump out of the `if`, and
            // allocate a new cluster. This limit exists to avoid unbounded memory use which can be
            // exploited by a malicious party to force an OOM crash.
            if state.uncompressed.len() < CLUSTER_CAPACITY {
                trace!(self, "extending existing cluster";
                       "old length" => state.uncompressed.len());

                // Extend the buffer of uncompressed data in the last allocated cluster.
                state.uncompressed.extend_from_slice(buf);

                // Check if we can compress the extended buffer into a single cluster.
                if let Some(compressed) = self.compress(state.uncompressed) {
                    let ptr = Ok(page::Pointer {
                        cluster: state.cluster,
                        // Calculate the offset into the decompressed buffer, where the page is
                        // stored.
                        offset: Some(state.uncompressed / disk::SECTOR_SIZE - 1),
                        checksum: cksum,
                    });

                    // Insert the page pointer into the deduplication table to allow future use as
                    // duplicate.
                    self.dedup_table.insert(buf, ptr);

                    // It succeeded! Write the compressed data into the cluster. Wrap the pointer
                    // in the transaction and return it.
                    return self.cache.write(state.cluster, compressed).wrap(ptr);
                }
            }
        });

        // We were unable to extend the last allocated cluster, either because there is no last
        // allocated cluster, or because the cluster could not contain the page. We'll allocate a
        // new cluster to contain our page.

        // Pop the cluster from the freelist.
        let cluster = self.freelist_pop()?;
        let ptr = if let Some(compressed) = self.compress(buf) {
            trace!(self, "storing compressible page in cluster"; "cluster" => cluster);

            // We were able to compress the page to fit into the cluster. At first, compressing the
            // first page seems unnecessary as it is guaranteed to fit in without compression, but
            // it has a purpose: namely that it allows us to extend the cluster. Enabling
            // compression in an uncompressed cluster is not plausible, as it would require
            // updating pointers pointing to the clujster. However, when we are already compressed,
            // there is no change in how the other pages are read.

            // Make the "last cluster" the newly allocated cluster.
            self.last_cluster.replace(Some(ClusterState {
                cluster: cluster,
                // So far, it only contains one page.
                uncompressed: buf.as_vec(),
            }));

            // Write the compressed data into the cluster.
            cluster.then(self.cache.write(cluster, compressed)).wrap(page::Pointer {
                cluster: cluster,
                offset: Some(0),
                checksum: cksum,
            })
        } else {
            trace!(self, "storing incompressible page in cluster"; "cluster" => cluster);

            // We were not able to compress the page into a single cluster. We work under the
            // assumption, that we cannot do so either when new data is added. This makes the
            // algorithm greedy, but it is a fairly reasonable assumption to make, as most
            // compression algorithm works at a stream level, and even those that don't (e.g.
            // algorithms with a reordering step), rarely shrinks by adding more data.

            // `self.last_cluster` will continue being `None`, until an actually extendible
            // (compressed) cluster comes in.

            // Write the data into the cluster, uncompressed.
            cluster.then(self.cache.write(cluster, buf)).replace_inner(page::Pointer {
                cluster: cluster,
                offset: None,
                checksum: cksum,
            })
        };

        // Insert the page pointer into the deduplication table to allow future use as
        // duplicate.
        self.dedup_table.insert(buf, ptr);

        Ok(ptr)
    }

    /// Read/dereference a page.
    ///
    /// This reads page `page` and returns the content.
    pub fn read(&self, page: page::Pointer) -> Result<disk::SectorBuf, Error> {
        trace!(self, "reading page"; "page" => page);

        // Read the cluster in which the page is stored.
        self.cache.read_then(page.cluster, |cluster| {
            // Decompress if necessary.
            let buf = if let Some(offset) = page.offset {
                // The page is compressed, decompress it and read at some offset `offset` (in pages).

                // Decompress the cluster.
                let decompressed = self.decompress(cluster)?;

                // Read the decompressed stream from some offset, into a sector buffer.
                let mut tmp = disk::SectorBuf::default();
                // TODO: Find a way to eliminate this memcpy.
                tmp.copy_from_slice(decompressed[offset * disk::SECTOR_SIZE..][..disk::SECTOR_SIZE]);

                tmp
            } else {
                // The page was not compressed so we can just use the cluster directly.
                cluster
            };

            // Check the data against the stored checksum.
            let cksum = self.checksum(buf) as u32;
            if cksum as u32 != page.checksum {
                // The checksums mismatched, thrown an error.
                return Err(Error::PageChecksumMismatch {
                    page: page,
                    found: cksum,
                });
            }

            Ok(ret)
        })
    }

    /// Calculate the checksum of some buffer, based on the user configuration.
    fn checksum(&self, buf: &disk::SectorBuf) -> u64 {
        trace!(self, "calculating checksum");

        self.driver.header.hash(buf)
    }

    /// Compress some data based on the compression configuration option.
    ///
    /// # Panics
    ///
    /// This will panic if compression is disabled.
    fn compress(&self, input: &[u8]) -> Option<disk::SectorBuf> {
        trace!(self, "compressing data");

        // Compress the input.
        let compressed = match self.config.compression_algorithm {
            // We'll panic if compression is disabled, as it is assumed that the caller handles
            // this case.
            CompressionAlgorithm::Identity => panic!("Compression was disabled."),
            // Compress via LZ4.
            CompressionAlgorithm::Lz4 => lz4_compress::compress(input),
        };

        if compressed.len() < disk::SECTOR_SIZE {
            // We were able to compress the input into at least one cluster. Now, we apply padding.

            // Write a delimiter to make the padding distinguishable from the actual data (e.g. if
            // it ends in zero).
            // TODO: This is not bijective. Very bad! FAKE NEWS
            compressed.push(0xFF);

            // Convert it to type `disk::SectorBuf`.
            let mut buf = disk::SectorBuf::default();
            // TODO: Find a way to eliminate this memcpy.
            buf[..compressed.len()].copy_from_slice(&compressed);
        } else {
            // We were unable to compress the input into one cluster.
            None
        }
    }

    /// Decompress some data based on the compression configuration option.
    ///
    /// # Panics
    ///
    /// This will panic if compression is disabled.
    fn decompress(&self, cluster: disk::SectorBuf) -> Result<Box<[u8]>, Error> {
        trace!(self, "decompressing data");

        // Find the padding delimited (i.e. the last non-zero byte).
        if let Some((len, _)) = cluster.enumerate().rev().find(|(_, x)| x != 0) {
            // We found the delimiter and can now distinguish padding from data.
            Ok(match self.config.compression_algorithm {
                // We'll panic if compression is disabled, as it is assumed that the caller handles
                // this case.
                CompressionAlgorithm::Identity => panic!("Compression was disabled."),
                // Decompress the non-padding section from LZ4.
                CompressionAlgorithm::Lz4 => lz4_compress::decompress(source[..len])?,
            })
        } else {
            // No delimiter was found, indicating data corruption.
            // TODO: Use a special error for this.
            Err(Error::InvalidCompression)
        }
    }

    /// Flush the state block.
    ///
    /// This flushes the state block (not to the disk, but to the cache), and returns the
    /// transaction.
    ///
    /// It takes a state in order to avoid re-acquiring the lock.
    fn flush_state_block(&mut self, state: &state_block::State) -> cache::Transaction {
        trace!(self, "flushing the state block to the cache");

        // Do it, motherfucker.
        self.cache.write(self.driver.header.state_block_address, state_block::StateBlock {
            config: self.config,
            state: state,
        }.encode())
    }

    /// Pop from the freelist.
    ///
    /// The returned pointer is wrapped in a cache transaction, representing the operations done in
    /// order to pop it.
    fn freelist_pop(&mut self) -> Result<cache::Transacting<cluster::Pointer>, Error> {
        trace!(self, "popping from freelist");

        if let Some(free) = self.free.pop() {
            // We had a cluster in the free-cache. We can simply return this, no transactions are
            // required.
            Transacting::no_transaction(free)
        } else {
            // We were unable to pop from the free-cache, so we must grab the next metacluster and
            // load it.

            // Lock the state.
            let state = self.state.lock();
            // Just in case that another thread have filled the free-cache while we were locking
            // the state, we will check if new clusters are in the free-cache.
            if Some(x) = self.free.pop() {
                // We had a cluster in the free-cache. Again, we can simply return this, no
                // transactions are required.
                return Transacting::no_transaction(free)
            }

            // Grab the next metacluster. If no other metacluster exists, we return an error.
            let head = state.freelist_head.ok_or(Error::OutOfClusters)?;
            // Load the new metacluster, and return the old metacluster.
            let free = self.cache.read_then(head.cluster, |buf| {
                // Check that the checksum matches.
                let found = self.checksum(buf);
                if head.checksum != found {
                    // Checksums do not match; throw an error.
                    return Err(Error::MetacluterChecksumMismatch {
                        cluster: head.cluster,
                        expected: head.checksum,
                        found: found,
                    });
                }

                // Now, we'll replace the old head metacluster with the chained metacluster.
                trace!(self, "metacluster checksum matched", "checksum" => found);

                // Replace the checksum of the head metacluster with the checksum of the chained
                // metacluster, which will soon be the head metacluster.
                head.checksum = LittleEndian::read(buf);
                // We'll initialize a window iterating over the pointers in this metacluster.
                let mut window = &buf[8..];
                // The first pointer points to the chained metacluster.
                let old_head = mem::replace(&mut head.cluster, cluster::Pointer::from(LittleEndian::read(window)));

                // The rest are free.
                while window.len() >= 8 {
                    // Slide the window to the right.
                    window = &window[8..];
                    // Read the pointer.
                    if let Some(ptr) = cluster::Pointer::from(LittleEndian::read(window)) {
                        // There was anohter pointer in the metacluster, push it to the free-cache.
                        self.free.push(ptr)
                    } else {
                        // The pointer was a null pointer, so the metacluster is over.
                        break;
                    }
                }

                // We return the old head metacluster, and will use it as the popped free cluster.
                // Mein gott, dis is incredibly convinient. *sniff*
                old_head
            });

            // Release the lock.
            drop(state)
            // Flush the state block to account for the changes.
            self.flush_state_block().wrap(free)
        }
    }

    /// Push to the freelist.
    fn freelist_push(&mut self, cluster: cluster::Pointer) -> cache::Transaction {
        trace!(self, "pushing to freelist"; "cluster" => cluster);

        self.free.push(cluster);
    }

    /// Flush the free-cache to the head metacluster.
    ///
    /// This clears the free-cache and writes it to the head metacluster.
    fn flush_free(&self) -> Transacting<()> {
        /* TODO (buggy and incomplete)
        let state = self.state.lock();
        let mut ret = Transacting::no_transaction();
        let mut (ptr, cksum) = state.freelist_head.map_or(|x| (x.cluster, x.checksum), (0, 0));

        let mut buf = disk::SectorBuf::default();
        let mut window = 8 + cluster::POINTER_SIZE;
        while let Some(free) = self.free.pop() {
            if window == disk::SECTOR_SIZE {
                LittleEndian::write(&mut buf, cksum);
                LittleEndian::write(&mut buf[8..], ptr);

                ptr = free;
                cksum = self.checksum(buf);

                ret = ret.then(self.cache.write(ptr, &buf));

                window = 8 + cluster::POINTER_SIZE;
            } else {
                LittleEndian::write(&mut buf[window..], x);
                window += cluster::POINTER_SIZE;
            }
        }

        state.freelist_head = Some(FreelistHead {
            cluster: ptr,
            checksum: cksum,
        });

        ret.then(self.flush_state_block())
        */
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        // Flush the free-cache before exiting.
        self.flush_free();
    }
}

delegate_log!(Manager.cache);
