//! Page management and cluster allocation.
//!
//! # Compression
//!
//! Pages are virtual data units of size 512 bytes. They're represented on disk somewhat
//! non-obviously, since clusters can hold more than one page at once (compression). Every cluster
//! will maximize the number of pages held and when it's filled up, a new cluster will be fetched.
//!
//! # Cluster allocator
//!
//! The allocator is a basic unrolled list of clusters.

mod dedup;
pub mod page;
pub mod state_block;

use crossbeam::sync::SegQueue;
use futures::{future, Future};
use std::mem;
use std::sync::atomic;
use disk::{self, cluster, Disk};
use {little_endian, lz4_compress, thread_object, Error};

/// The atomic ordering used in the allocator.
const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;
/// The maximal number of clusters in a freelist node.
const CLUSTERS_IN_FREELIST_NODE: u64 = disk::SECTOR_SIZE / cluster::POINTER_SIZE - 1;
// We subtract 1 to account for checksum.

/// Allocator options.
///
/// When an allocator system is provided, the user must provide some option, so they can adjust the
/// behavior to their needs. This struct contain the parameters used to construct the allocator
/// system.
struct Options {
    /// The options from the state block.
    state_block: state_block::Options,
    /// The options from the disk header.
    disk_header: disk::header::Options,

    // In the future, allocator specific options may be added here.
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

/// The page allocator system.
///
/// This is the center point of the I/O stack, providing allocation, deallocation, compression,
/// etc. It manages the clusters (with the page abstraction) and caches the disks.
pub struct Allocator<D> {
    /// The inner disk cache.
    cache: disk::TfsDisk<D>,
    /// The on-disk state.
    ///
    /// This is the state as stored in the state block. The reason we do not store the whole state
    /// block in one is that, we want to avoid the lock when reading the static parts of the state
    /// block (e.g. configuration).
    state: conc::sync::Stm<state_block::State>,
    /// The configuration options.
    ///
    /// This is the configuration part of the state block. We don't need a lock, since we won't
    /// mutate it while the system is initialized.
    options: state_block::Options,
    /// The free-cache.
    ///
    /// This contains some number of pointers to free clusters, allowing multiple threads to
    /// efficiently allocate simultaneously.
    free: conc::sync::Treiber<cluster::Pointer>,
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

impl<D: Disk> Allocator<D> {
    /// Open the manager from some disk.
    ///
    /// This future creates a future, which loads the state page and other things from a the disk
    /// `disk`. If it fails, the future will return an error.
    pub fn open(disk: D) -> future!(Allocator<D>) {
        // Initialize the disk and cache.
        let cache = disk::open(disk);
        // Read the state block.
        cache.read(0).map(|state_block| {
            // Parse the state block.
            let state_block::StateBlock { state, options } =
                state_block::StateBlock::decode(state_block, cache.disk_header().checksum_algorithm);

            // I'm sure you're smart enough to figure out what is happening here. I trust you ^^.
            Allocator {
                cache: cache,
                state: conc::sync::Stm::new(state),
                options: options,
                free: SegQueue::new(),
                last_cluster: thread_object::Object::default(),
                dedup_table: dedup::Table::default(),
            }
        })
    }

    /// Initialize a new system given a set of options.
    ///
    /// This uses the parameters from `options` to initialize a new, empty system on the disk
    /// `disk`. This doesn't open the disk, as `Allocator::open()` does: Instead it creates a new
    /// fresh system, ignoring the existing data.
    ///
    /// The initialization is complete when the returned future completes.
    pub fn init(disk: D, options: Options) -> future!(Allocator<D>) {
        unimplemented!();

        // Initialize the disk (below the allocator stack).
        disk::init(disk, options.disk_header).and_then(|cache| {
            // Write the state block to the start of the disk.
            cache.write(0, options.state_block.encode()).map(|_| cache)
        }).map(|cache| Allocator {
            cache: cache,
            state: conc::sync::Stm::new(state),
            options: options.state_block,
            free: SegQueue::new(),
            last_cluster: thread_object::Object::default(),
            dedup_table: dedup::Table::default(),
        })
    }

    /// Allocate a page in a new cluster.
    ///
    /// This allocates a new cluster and uses that to store the page. It will not try to extend the
    /// last used cluster, but it will set the last used cluster (which is provided as a mutable
    /// reference in the `last_cluster` argument) to the allocated cluster, if it was compressed.
    /// Naturally, the result is wrapped in a future.
    ///
    /// `cksum` is assumed to be the checksum of `buf` through the algorithm from
    /// `self.checksum()`.
    ///
    /// This **does not** update the deduplication table.
    ///
    /// # Panics
    ///
    /// This will panic if compression is disabled.
    fn alloc_in_new_cluster(
        &self,
        buf: Box<disk::SectorBuf>,
        last_cluster: &mut Option<ClusterState>,
        cksum: u32,
    ) -> future!(page::Pointer) {
        // Pop the cluster from the freelist, then attempt to compress the data.
        self.freelist_pop().and_then(|cluster| if let Some(compressed) = self.compress(buf) {
            // We were able to compress the page to fit into the cluster. At first, compressing the
            // first page seems unnecessary as it is guaranteed to fit in without compression, but
            // it has a purpose: namely that it allows us to extend the cluster. Enabling
            // compression in an uncompressed cluster is not plausible, as it would require
            // updating pointers pointing to the cluster.  However, when we are already compressed,
            // there is no change in how the other pages are read.
            trace!(self, "storing compressible page in cluster"; "cluster" => cluster);

            // Update the "last cluster" state variable to point to the new cluster.
            last_cluster = Some(ClusterState {
                cluster: cluster,
                // So far, it only contains one page.
                uncompressed: buf.as_vec(),
            });

            // Write the compressed data into the cluster.
            self.cache.write(cluster, compressed).map(|_| page::Pointer {
                cluster: cluster,
                offset: Some(0),
                checksum: cksum,
            })
        } else {
            // We were not able to compress the page into a single cluster. We work under the
            // assumption, that we cannot do so either when new data is added. This makes the
            // algorithm greedy, but it is a fairly reasonable assumption to make, as most
            // compression algorithm works at a stream level, and even those that don't (e.g.
            // algorithms with a reordering step), rarely shrinks by adding more data.
            trace!(self, "storing incompressible page in cluster"; "cluster" => cluster);

            // `last_cluster` will continue having its value, until an actually extendible
            // (compressed) cluster comes in. This could mean that it an allocation which can be
            // appended to the last allocated cluster, or it could mean that an allocation, which
            // is compressible into one cluster comes in.

            // Write the data into the cluster, uncompressed.
            self.cache.write(cluster, buf).map(|_| page::Pointer {
                cluster: cluster,
                // It is very important that we don't use e.g. `Some(0)`, because this cluster is
                // not compressed.
                offset: None,
                checksum: cksum,
            })
        })
    }

    /// Allocate a page eagerly and without deduplication.
    ///
    /// This allocates buffer `buf` with checksum (as calculated by `self.checksum()`) `cksum`, and
    /// returns the page pointer wrapped in a future.
    ///
    /// This **does not** update the deduplication table, nor does it try to look for duplicates.
    /// Futhermore, some of the logic acts eagerly, and thus it ought to be wrapped in
    /// `future::lazy()` to avoid it being out of sequence.
    fn alloc_eager(
        &self,
        buf: Box<disk::SectorBuf>,
        cksum: u32,
    ) -> future!(page::Pointer) {
        // Handle the case where compression is disabled.
        if self.options.compression_algorithm == state_block::CompressionAlgorithm::Identity {
            // Pop a cluster from the freelist.
            return self.freelist_pop()
                // Write the cluster with the raw, uncompressed data.
                .and_then(|cluster| self.cache.write(cluster, buf).map(|_| cluster))
                .map(|cluster| page::Pointer {
                    cluster: cluster,
                    offset: None,
                    checksum: cksum,
                });
        }

        // If you have followed this path, compression is enabled (we won't use `else` in order to
        // flatten the code).

        self.last_cluster.with(|last_cluster| {
            if let Some(state) = last_cluster {
                // We have earlier allocated a cluster, meaning that we can potentially append
                // more pages into the cluster.
                trace!(self, "extending existing cluster";
                       "old length" => state.uncompressed.len());

                // Extend the buffer of uncompressed data in the last allocated cluster.
                state.uncompressed.extend_from_slice(buf);

                // Try to compress the extended buffer into a single cluster.
                if let Some(compressed) = self.compress(state.uncompressed) {
                    // It succeeded! Write the compressed data into the cluster.
                    return self.cache.write(state.cluster, compressed).map(|_| page::Pointer {
                        cluster: state.cluster,
                        // The offset is determined by simple division to get the number of
                        // sectors the uncompressed buffer spans.
                        offset: Some(state.uncompressed.len() / disk::SECTOR_SIZE),
                        checksum: cksum,
                    });
                }
            }

            // We were unable to extend the last allocated cluster, either because there is no
            // last allocated cluster, or because the cluster could not contain the page. We'll
            // allocate a new cluster to contain our page.
            self.alloc_in_new_cluster(buf, last_cluster)
        })
    }

    /// Allocate a page.
    ///
    /// This creates a future, which allocates a page with content `buf`. The content of the future
    /// is the page pointer to the page which was allocated.
    ///
    /// The algorithm works greedily by fitting as many pages as possible into the most recently
    /// used cluster.
    pub fn alloc(&mut self, buf: Box<disk::SectorBuf>) -> future!(page::Pointer) {
        // TODO: The variables are named things like `ptr`, which kinda contradicts the style of
        //       the rest of the code.

        // Calculate the checksum of the buffer. We'll use this later.
        let cksum = self.checksum(buf) as u32;
        debug!(self, "allocating page"; "checksum" => cksum);

        // Check if duplicate exists. This isn't wrapped in a future, because it isn't an I/O
        // operation.
        if let Some(page) = self.dedup_table.dedup(buf, cksum) {
            debug!(self, "found duplicate page"; "page" => page);
            // Deduplicate and simply use the already stored page.
            return Ok(page);
        }

        // To make sure the operations are executed in correct sequence (directly after each
        // other), we use a lazy evaluated future.
        future::lazy(|| {
            // Do the core of the allocation.
            self.alloc_eager(buf, cksum)
        }).map(|page| {
            // Insert the page pointer into the deduplication table to allow future use as
            // duplicate.
            self.dedup_table.insert(buf, page);

            // Return the allocated pointer.
            page
        })
    }

    /// Read/dereference a page.
    ///
    /// This reads page `page` and returns the content, wrapped in a future.
    pub fn read(
        &self,
        page: page::Pointer,
    ) -> future!(atomic_hash_map::Value<disk::SectorBuf>) {
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
                Err(err!(Corruption, "mismatching checksums in {} - expected {:x}, found {:x}",
                         page, page.checksum, cksum));
            } else {
                Ok(buf)
            }
        })
    }

    /// Calculate the checksum of some buffer, based on the user choice.
    fn checksum(&self, buf: &disk::SectorBuf) -> u64 {
        trace!(self, "calculating checksum");

        self.cache.disk_header().hash(buf)
    }

    /// Compress some data based on the compression option.
    ///
    /// # Panics
    ///
    /// This will panic if compression is disabled.
    fn compress(&self, input: &[u8]) -> Option<Box<disk::SectorBuf>> {
        trace!(self, "compressing data");

        // Compress the input.
        let compressed = match self.options.compression_algorithm {
            // We'll panic if compression is disabled, as it is assumed that the caller handles
            // this case.
            state_block::CompressionAlgorithm::Identity => panic!("Compression was disabled."),
            // Compress via LZ4.
            state_block::CompressionAlgorithm::Lz4 => lz4_compress::compress(input),
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

    /// Decompress some data based on the compression option.
    ///
    /// # Panics
    ///
    /// This will panic if compression is disabled.
    fn decompress(&self, cluster: Box<disk::SectorBuf>) -> Result<Box<[u8]>, Error> {
        trace!(self, "decompressing data");

        // Find the padding delimited (i.e. the last non-zero byte).
        if let Some((len, _)) = cluster.enumerate().rev().find(|(_, x)| x != 0) {
            // We found the delimiter and can now distinguish padding from data.
            Ok(match self.options.compression_algorithm {
                // We'll panic if compression is disabled, as it is assumed that the caller handles
                // this case.
                state_block::CompressionAlgorithm::Identity => panic!("Compression was disabled."),
                // Decompress the non-padding section from LZ4.
                state_block::CompressionAlgorithm::Lz4 => lz4_compress::decompress(cluster[..len])?,
            })
        } else {
            // No delimiter was found, indicating data corruption.
            // TODO: Provide the sector number.
            Err(err!(Corruption, "invalid compression"))
        }
    }

    /// Flush the state block.
    ///
    /// This creates a future, which will flush the state block when executed.
    ///
    /// It takes a mutable reference to the state in order to avoid clogging up the transaction and
    /// flushing asynchronized.
    fn flush_state_block(&mut self, state: &mut state_block::State) -> future!(()) {
        trace!(self, "flushing the state block");

        // Encode and write to virtual sector 0, the state block's sector.
        self.cache.write(0, state_block::StateBlock {
            options: self.options,
            state: state,
        }.encode())
    }

    /// Pop from the freelist.
    ///
    /// This returns a future, which wraps a cluster pointer popped from the freelist.
    fn freelist_pop(&mut self) -> future!(page::Pointer) {
        // In order to avoid eager evaluation (and potentially prematurely exhausting the
        // freelist), we use lazy popping by constructing the future when evaluated.
        future::lazy(|| {
            trace!(self, "popping from freelist");

            if let Some(free) = self.free.pop() {
                // We had a cluster in the free-cache.
                free
            } else {
                // We were unable to pop from the free-cache, so we must grab the next metacluster
                // and load it.

                // Lock the state.
                self.state.with(|state| {
                    // Grab the next metacluster. If no other metacluster exists, we return an
                    // error.
                    let head = state.freelist_head.ok_or(err!(OutOfSpace, "out of free clusters"))?;
                    // Load the new metacluster, and return the old metacluster.
                    self.cache.read_then(head.cluster, |buf| {
                        // Check that the checksum matches.
                        let found = self.checksum(buf);
                        if head.checksum != found {
                            // Checksums do not match; throw an error.
                            return Err(err!(Corruption, "mismatching checksums in metacluster {:x} \
                                            - expected {:x}, found {:x}", head.cluster,
                                            head.checksum, found));
                        }

                        // Now, we'll replace the old head metacluster with the chained
                        // metacluster.
                        trace!(self, "metacluster checksum matched"; "checksum" => found);

                        // Replace the checksum of the head metacluster with the checksum of the
                        // chained metacluster, which will soon be the head metacluster.
                        head.checksum = little_endian::read(buf);
                        // We'll initialize a window iterating over the pointers in this
                        // metacluster.
                        let mut window = &buf[cluster::POINTER_SIZE..];
                        // The first pointer points to the chained metacluster.
                        let old_head = mem::replace(&mut head.cluster, little_endian::read(window));

                        // It is absolutely crucial that we don't simply push directly to
                        // `self.free` as we're in an atomic transaction, which can potentially be
                        // run multiple times. Hence, such behavior could cause weird bugs such as
                        // double free.
                        let mut free = Vec::with_capacity(CLUSTERS_IN_FREELIST_NODE);
                        // The rest are free.
                        while window.len() >= 8 {
                            // Slide the window to the right.
                            window = &window[cluster::POINTER_SIZE..];
                            // Read the pointer.
                            if let Some(cluster) = little_endian::read(window) {
                                // There was anohter pointer in the metacluster, push it to the
                                // free-cache.
                                free.push(cluster)
                            } else {
                                // The pointer was a null pointer, so the metacluster is over.
                                break;
                            }
                        }

                        // Finally we push the old head to the vector, as it is free now.
                        free.push(old_head);

                        // Trim the old metacluster.
                        self.cache.trim(old_head).map(|_| free)
                    })
                }).and_then(|free| {
                    // Finally, we must flush the state block before we can add the found clutters
                    // to the free cache.
                    // FIXME: The `map` here is ugly.
                    self.flush_state_block(state).map(|_| free)
                }).map(|free| {
                    // At this point, the transaction have run and the state block is flushed.

                    // Push every (except one) element of our temporary vector of free clusters.
                    for i in free[1..] {
                        self.free.push(i);
                    }

                    // We use the last cluster as the popped cluster.
                    free[0]
                })
            }
        })
    }

    /// Push to the freelist.
    ///
    /// No I/O logic happens, since pushes are buffered.
    fn freelist_push(&mut self, cluster: cluster::Pointer) {
        trace!(self, "pushing to freelist"; "cluster" => cluster);

        // Push the cluster to the freelist.
        self.free.push(cluster);
    }

    pub fn flush_free(&self) {
        // TODO: Important!! Remember to trim the sectors which gets dealloc'd. This can be done
        //       through `self.cache.trim`

        /* TODO (buggy and incomplete)
        let state = self.state.lock();
        let mut (cluster, cksum) = state.freelist_head.map_or(|x| (x.cluster, x.checksum), (0, 0));

        let mut buf = disk::SectorBuf::default();
        let mut window = 8 + cluster::POINTER_SIZE;
        while let Some(free) = self.free.pop() {
            if window == disk::SECTOR_SIZE {
                little_endian::write(&mut buf, cksum);
                little_endian::write(&mut buf[cluster::POINTER_SIZE..], cluster);

                cluster = free;
                cksum = self.checksum(buf);

                self.cache.write(cluster, &buf)?;

                window = 8 + cluster::POINTER_SIZE;
            } else {
                little_endian::write(&mut buf[window..], x);
                window += cluster::POINTER_SIZE;
            }
        }

        state.freelist_head = Some(FreelistHead {
            cluster: cluster,
            checksum: cksum,
        });

        self.flush_state_block()
        */
    }
}

impl<D: Disk> Drop for Allocator<D> {
    fn drop(&mut self) {
        // Flush the buffered free clusters to avoid leaking space.
        self.flush_free();
    }
}

delegate_log!(Allocator.cache);
