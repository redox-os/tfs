//! Virtual devices.
//!
//! A virtual device or "vdev" is a disk with some extra capabilities. It transforms operations to
//! other operationss in order to provide new features.
//!
//! Vdevs themself can be seen as an image (transformation) of another disk. They might modify the
//! sector enumeration or provide some redundancy, encryption, or similar features working on disk
//! level.
//!
//! The term vdev has a equivalent meaning in the context of ZFS.
//!
//! It is important that vdevs keep the invariants of the inner vdev. In particular, it may not
//! leave to an inconsistent state, unless the inner vdev does. Futhermore, the disk header must be
//! in sector 0, unmodified.

/// A mirror vdev.
///
/// This takes the inner disk, divides it into two equally sized parts, and makes the higher half a
/// mirror (exact copy) of the higher part.
///
/// It allows you to potentially heal a sector, by fetching its copy in the higher part.
struct Mirror {
    /// The inner disk.
    inner: Box<Disk>,
}

impl Disk for Mirror {
    fn number_of_sectors(&self) -> disk::Sector {
        // Simply half the number of sectors of the inner disk.
        self.inner.number_of_sectors() >> 1
    }

    fn write(sector: Sector, buf: &SectorBuf) -> Result<(), disk::Error> {
        // Write to the main sector.
        self.inner.write(sector, buf)?;
        // Write the mirror sector. If the disk was somehow suspended before the mirror sector was
        // written, both halfs of the inner disk are still consistent, so it won't leave to an
        // inconsistent state, despite being out of sync.
        self.inner.write(sector + self.number_of_sectors(), buf)
    }
    fn read(sector: Sector, buf: &mut SectorBuf) -> Result<(), disk::Error> {
        // Forward the call to the inner.
        self.inner.read(sector, buf)
    }

    fn heal(&mut self, sector: disk::Sector) -> Result<(), disk::Error> {
        // Get the size of half of the inner disk.
        let half = self.number_of_sectors();
        // Check if in bound.
        if sector < half {
            // Read the mirrored sector from the higher half, in the hope that it isn't corrupt as
            // well.
            let mut buf = disk::SectorBuf::default();
            self.read(sector + half, &mut buf)?;

            // Write the mirror sector the healing sector.
            self.write(sector, &buf)
        } else {
            // Out of bounds sector; throw an error.
            Err(disk::Error::OutOfBounds {
                sector: sector,
            })
        }
    }
}

/// A SPECK encryption vdev.
///
/// This encrypts the inner disk with the SPECK cipher in XEX mode and Scrypt key stretching.
struct Speck {
    /// The inner disk.
    inner: Box<Disk>,
    /// The key to encrypt with.
    ///
    /// This is derived through Scrypt.
    key: u128,
}

impl Disk for Speck {
    fn number_of_sectors(&self) -> disk::Sector {
        // Simply forward the call to the inner disk.
        self.inner.number_of_sectors()
    }

    fn write(sector: Sector, buf: &SectorBuf) -> Result<(), disk::Error> {
        // TODO: implement. NB: Dont' encrypt it if it is the header!
        unimplemented!()
    }
    fn read(sector: Sector, buf: &mut SectorBuf) -> Result<(), disk::Error> {
        unimplemented!()
    }

    fn heal(&mut self, sector: disk::Sector) -> Result<(), disk::Error> {
        // Simply forward the call to the inner disk.
        self.inner.heal(sector)
    }
}

quick_error! {
    /// A driver loading error.
    enum Error {
        /// The state flag was set to "inconsistent".
        InconsistentState {
            description("The state flag is marked inconsistent.")
        }
        /// A disk header parsing error.
        Parse(err: ParseError) {
            from()
            description("Disk header parsing error")
            display("Disk header parsing error: {}", err)
        }
        /// A disk error.
        Disk(err: disk::Error) {
            from()
            description("Disk I/O error")
            display("Disk I/O error: {}", err)
        }
    }
}

/// A driver transforming a normal disk into a disk respecting the vdev setup.
///
/// It reads the vdev setup from the disk header, which it fetches from the disk. Then it builds
/// the vdev stack, which it stores.
///
/// Note that it doesn't subtract the disk header sector, since the null sector can still be used
/// as a trap value, but reading or writing from it results in panic.
struct Driver<L> {
    /// The log exitpoint.
    pub log: L,
    /// The cached disk header.
    ///
    /// The disk header contains various very basic information about the disk and how to interact
    /// with it.
    ///
    /// In reality, we could fetch this from the `disk` field as-we-go, but that hurts performance,
    /// so we cache it in memory.
    pub header: header::DiskHeader,
    /// The inner disk.
    // TODO: Remove this vtable?
    disk: Box<Disk>,
}


impl<L: slog::Drain> Driver<L> {
    /// Set up the driver from some disk.
    ///
    /// This will load the disk header from `disk` and construct the driver. It will also set the
    /// disk to be in open state. If any encryption is enabled, `password` will be used as the
    /// password.
    fn open<T: Disk>(log: L, disk: T, password: &[u8]) -> Result<Driver, Error> {
        info!(log, "initializing the driver");

        // Load the disk header into some buffer.
        debug!(log, "reading the disk header");
        let mut header_buf = [0; disk::SECTOR_SIZE];
        disk.read(0, &mut header_buf)?;

        // Decode the disk header.
        debug!(log, "decoding the disk header");
        let mut header = DiskHeader::decode(header_buf)?;

        match header.state_flag {
            // Throw a warning if it wasn't properly shut down.
            StateFlag::Open => {
                warn!(log, "the disk's state flag is still open, likely wasn't properly shut down \
                            last time; beware of data loss");
            },
            // The state inconsistent; throw an error.
            StateFlag::Inconsistent => return Err(OpenError::InconsistentState),
        }

        // Set the state flag to open.
        debug!(log, "setting the state flag to 'open'");
        header.state_flag = StateFlag::Open;

        // Update the version.
        debug!(log, "updating the version number";
               "old version" => header.version_number,
               "new version" => VERSION_NUMBER);
        header.version_number = VERSION_NUMBER;

        // Construct the vdev stack.
        let mut disk = Box::new(disk);
        for i in header.vdev_stack {
            disk = match i {
                Vdev::Mirror => {
                    debug!(log, "appending a mirror vdev");

                    Box::new(Mirror {
                        inner: disk,
                    })
                },
                Vdev::Speck { salt } => {
                    debug!(log, "appending a SPECK encryption vdev", "salt" => salt);

                    Box::new(Speck {
                        inner: disk,
                        // Derive the key.
                        key: crypto::derive(salt, password),
                    })
                },
            };
        }

        let driver = Driver {
            header: header,
            disk: disk,
        };

        // Flush the updated header.
        driver.flush_header()?;

        Ok(driver)
    }

    /// Flush the stored disk header.
    fn flush_header(&mut self) -> Result<(), disk::Error> {
        debug!(self, "flushing the disk header");

        // Encode and write it to the disk.
        self.disk.write(0, &self.header.encode())
    }
}

impl<L: slog::Drain> Drop for Driver<L> {
    fn drop(&mut self) {
        info!(self, "closing the driver");

        // Set the state flag to close so we know that it was a proper shutdown.
        debug!(self, "setting state flag to 'closed'");
        self.header.state_flag = StateFlag::Closed;
        // Flush the header.
        self.flush_header();
    }
}

delegate_log!(Driver.log);

impl<L: slog::Drain> Disk for Driver<L> {
    fn number_of_sectors(&self) -> Sector {
        self.disk.number_of_sectors()
    }

    fn write(sector: Sector, buf: &[u8]) -> Result<(), Error> {
        trace!("writing data"; "sector" => sector);

        // Make sure it doesn't write to the null sector reserved for the disk header.
        assert_ne!(sector, 0, "Trying to write to the null sector.");

        // Forward the call to the inner disk.
        self.disk.write(sector, buf)
    }
    fn read(sector: Sector, buf: &mut [u8]) -> Result<(), Error> {
        trace!("reading data"; "sector" => sector);

        // Make sure it doesn't write to the null sector reserved for the disk header.
        assert_ne!(sector, 0, "Trying to read from the null sector.");

        // Forward the call to the inner disk.
        self.disk.read(sector, buf)
    }

    fn heal(&mut self, sector: disk::Sector) -> Result<(), disk::Error> {
        debug!("healing possibly corrupt sector"; "sector" => sector);

        // Forward the call to the inner disk.
        self.disk.heal(sector)
    }
}
