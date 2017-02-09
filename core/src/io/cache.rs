/// The default initial capacity of the sector map.
const INITIAL_CAPACITY: usize = 256;

/// A cached disk.
///
/// This wrapper manages caching and the consistency issues originating from it.
struct Cache {
    /// The inner driver.
    driver: vdev::Driver,

    /// The cache replacement tracker.
    ///
    /// This tracks the state of the replacement algorithm, which chooses which cache block shall
    /// be replaced in favor of a new cache. It serves to estimate/guess which block is likely not
    /// used in the near future.
    tracker: mlcr::ConcurrentCache,
    /// The sector-number-to-data block map.
    sectors: CHashMap<disk::Sector, disk::SectorBuf>,
}

impl From<vdev::Driver> for Cache {
    fn from(driver: vdev::Driver) -> Cache {
        Cache {
            // Store the driver.
            driver: driver,
            // Set empty/default values.
            tracker: mlcr::ConcurrentCache::new(),
            sectors: CHashMap::with_capacity(INITIAL_CAPACITY),
        }
    }
}

impl Cache {
    /// Write a sector.
    ///
    /// This writes `buf` into sector `sector`. If it fails, the error is returned.
    fn write(&self, sector: disk::Sector, buf: disk::SectorBuf) -> Result<(), disk::Error> {
        debug!(self, "writing sector"; "sector" => sector);

        // Write the data to the disk.
        self.driver.write(sector, &buf)?;
        // Then insert it into the cache.
        self.sectors.insert(sector, buf);
    }

    /// Drop a sector from the cache.
    fn forget(&self, sector: disk::Sector) {
        debug!(self, "removing sector from cache"; "sector" => sector);

        // Update the cache tracker.
        self.tracker.remove(sector);
        // Update the sector map.
        self.sectors.remove(sector);
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
        if let Some(accessor) = self.sectors.find(sector) {
            // Yup, we found the sector in the cache.
            trace!(self, "cache hit; reading from cache"; "sector" => sector);

            // Touch the sector.
            self.tracker.touch(sector);

            handler(accessor)
        } else {
            trace!(self, "cache miss; reading from disk"; "sector" => sector);

            // Occupy the block in the map, so that we can later on insert it.
            let block = self.sectors.get_mut_or(sector, Block::default());
            // Insert the sector into the cache tracker.
            self.tracker.touch(sector);

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
    /// This reduces the cache to exactly `to` blocks.
    fn trim(&self, to: usize) {
        info!(self, "trimming cache"; "to" => to);

        // Lock the cache tracker.
        let tracker = self.tracker.lock();

        // Remove all the coldest sectors.
        for i in tracker.trim(to) {
            // Remove that piece of shit.
            self.sectors.remove(i);
        }
    }
}

delegate_log!(Cache.driver);

// TODO: Add tests.
