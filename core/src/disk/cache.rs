use futures::Future;
use atomic_hashmap::AtomicHashMap;
use {mlcr, Error};
use disk::{self, vdev, Disk};
use disk::header::DiskHeader;

/// The default initial capacity of the sector map.
const INITIAL_CAPACITY: usize = 256;

/// A cached disk.
///
/// This wrapper manages caching of the disk.
pub struct Cached<D> {
    /// The inner disk.
    disk: D,

    /// The cache replacement tracker.
    ///
    /// This tracks the state of the replacement algorithm, which chooses which cache block shall
    /// be replaced in favor of a new cache. It serves to estimate/guess which block is likely not
    /// used in the near future.
    tracker: mlcr::ConcurrentCache,
    /// The sector-number-to-data block map.
    sectors: AtomicHashMap<disk::Sector, disk::SectorBuf>,
}

impl<D: Disk> Cached<D> {
    /// Create a cache from a backing disk.
    fn new(disk: D) -> Cached<D> {
        Cached {
            disk: disk,
            tracker: mlcr::ConcurrentCache::new(),
            sectors: AtomicHashMap::with_capacity(INITIAL_CAPACITY),
        }
    }

    /// Write a sector.
    ///
    /// This writes `buf` into sector `sector`. If it fails, the error is returned.
    fn write(
        &self,
        sector: disk::Sector,
        buf: Box<disk::SectorBuf>,
    ) -> future!(()) {
        debug!(self, "writing sector"; "sector" => sector);

        // Then insert it into the cache.
        self.sectors.insert(sector, buf);
        // Write the data to the disk.
        self.disk.write(sector, &buf)
    }

    /// Drop a sector from the cache and trim it.
    ///
    /// After this has been completed, the data of the sector shall not be read, until some other
    /// data has been written to the sector.
    ///
    /// Note that it doesn't necessarily "wipe" the data.
    fn trim(&self, sector: disk::Sector) -> future!(()) {
        debug!(self, "wiping sector"; "sector" => sector);

        // Update the cache tracker.
        self.tracker.remove(sector);
        // Update the sector map.
        self.sectors.remove(sector);
        // Finally, trim the sector.
        self.disk.trim(sector)
    }

    /// Read a sector.
    ///
    /// This reads sector `sector`, and applies the closure `map`. If `sector` needs to be fetched
    /// from the disk, and `map` fails, data recovery is attempted.
    ///
    /// If an I/O operation fails, the error is returned. Otherwise, the return value of `map` is
    /// returned.
    fn read_then<F, T>(&self, sector: disk::Sector, map: F) -> future!(T)
    where F: Fn(atomic_hash_map::Value<disk::SectorBuf>) -> future!(T) {
        debug!(self, "reading sector"; "sector" => sector);

        // Check if the sector is already available in the cache.
        if let Some(buf) = self.sectors.get(sector) {
            // Yup, we found the sector in the cache.
            trace!(self, "cache hit; reading from cache"; "sector" => sector);

            // Touch the sector.
            self.tracker.touch(sector);

            map(buf)
        } else {
            trace!(self, "cache miss; reading from disk"; "sector" => sector);

            // Insert the sector into the cache tracker.
            self.tracker.touch(sector);

            // Fetch the data from the disk.
            self.disk.read(sector).map(|buf| {
                // Insert the read data into the hash table.
                self.sectors.get_mut_or(sector, buf)
            }).and_then(map)
            // TODO: If the above failed, try to recover the data through the vdev redundancy.
        }
    }

    /// Reduce the cache.
    ///
    /// This reduces the cache to exactly `to` blocks.
    fn reduce(&self, to: usize) {
        info!(self, "reducing cache"; "to" => to);

        // Lock the cache tracker.
        let tracker = self.tracker.lock();

        // Remove all the coldest sectors.
        for i in tracker.trim(to) {
            // Remove that piece of shit.
            self.sectors.remove(i);
        }
    }
}

delegate_log!(Cached.disk);

// TODO: Add tests.
