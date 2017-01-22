/// A virtual device.
///
/// A virtual device or "vdev" is a disk with some extra capabilities. It transforms operations to
/// disks to provide new features.
///
/// Vdevs themself can be seen as an image (transformation) of another vdev. They might modify the
/// sector enumeration or provide some redundancy, encryption, or similar features.
///
/// The term vdev has a equivalent meaning in the context of ZFS.
///
/// It is important that vdevs keep the invariants of the inner vdev. In particular, it may not
/// leave to an inconsistent state, unless the inner vdev does. Futhermore, the disk header must be
/// in sector 0, unmodified.
trait Vdev: Disk {
    /// Heal a sector.
    ///
    /// This heals sector `sector`, through the provided redundancy, if possible.
    ///
    /// Note that after it is called, it is still necessary to check if the healed sector is valid,
    /// as there is a certain probability that the recovery will fail.
    fn heal(&mut self, sector: disk::Sector) -> Result<(), disk::Error>;
}

/// A mirror vdev.
///
/// This takes the inner disk, divides it into two equally sized parts, and makes the higher half a
/// mirror (exact copy) of the higher part.
///
/// It allows you to potentially heal a sector, by fetching its copy in the higher part.
struct Mirror {
    /// The inner vdev.
    inner: Box<Vdev>,
}

impl Vdev for Mirror {
    fn heal(&mut self, sector: disk::Sector) -> Result<(), disk::Error> {
        // Get the size of half of the inner vdev.
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

impl Disk for Mirror {
    fn number_of_sectors(&self) -> disk::Sector {
        // Simply half the number of sectors of the inner vdev.
        self.inner.number_of_sectors() >> 1
    }

    fn write(sector: Sector, buf: &SectorBuf) -> Result<(), disk::Error> {
        // Write to the main sector.
        self.inner.write(sector, buf)?;
        // Write the mirror sector. If the disk was somehow suspended before the mirror sector was
        // written, both halfs of the inner vdev are still consistent, so it won't leave to an
        // inconsistent state, despite being out of sync.
        self.inner.write(sector + self.number_of_sectors(), buf)
    }
    fn read(sector: Sector, buf: &mut SectorBuf) -> Result<(), disk::Error> {
        // Forward the call to the inner.
        self.inner.read(sector, buf)
    }
}

/// A SPECK encryption vdev.
///
/// This encrypts the inner vdev with the SPECK cipher in XEX mode and Scrypt key stretching.
struct Speck {
    /// The inner vdev.
    inner: Box<Vdev>,
    /// The key to encrypt with.
    ///
    /// This is derived through Scrypt.
    key: u128,
}

impl Vdev for Speck {
    fn heal(&mut self, sector: disk::Sector) -> Result<(), disk::Error> {
        // Simply forward the call to the inner vdev.
        self.inner.heal(sector)
    }
}

impl Disk for Speck {
    fn number_of_sectors(&self) -> disk::Sector {
        // Simply forward the call to the inner vdev.
        self.inner.number_of_sectors()
    }

    fn write(sector: Sector, buf: &SectorBuf) -> Result<(), disk::Error> {
        // TODO: implement. NB: Dont' encrypt it if it is the header!
        unimplemented!()
    }
    fn read(sector: Sector, buf: &mut SectorBuf) -> Result<(), disk::Error> {
        unimplemented!()
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
struct Driver {
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
    vdev: Box<Vdev>,
}


impl Driver {
    /// Set up the driver from some disk.
    ///
    /// This will load the disk header from `disk` and construct the driver. It will also set the
    /// disk to be in open state. If any encryption is enabled, `password` will be used as the
    /// password.
    fn open<T: Disk>(disk: T, password: &[u8]) -> Result<Driver, Error> {
        // Load the disk header into some buffer.
        let mut header_buf = [0; disk::SECTOR_SIZE];
        disk.read(0, &mut header_buf)?;

        // Decode the disk header.
        let mut header = DiskHeader::decode(header_buf)?;

        // TODO: Throw a warning if the flag is still in loading state.
        match header.state_flag {
            // Set the state flag to open.
            StateFlag::Closed => header.state_flag = StateFlag::Open,
            // The state inconsistent; throw an error.
            StateFlag::Inconsistent => return Err(OpenError::InconsistentState),
        }

        // Update the version.
        header.version_number = VERSION_NUMBER;

        // Construct the vdev stack.
        let mut vdev = Box::new(vdev);
        for i in header.vdev_stack {
            vdev = match i {
                Vdev::Mirror => Box::new(Mirror {
                    inner: vdev,
                }),
                Vdev::Speck { salt } => Box::new(Speck {
                    inner: vdev,
                    // Derive the key.
                    key: crypto::derive(salt, password),
                }),
            };
        }

        let driver = Driver {
            header: header,
            vdev: vdev,
        };

        // Flush the updated header.
        driver.flush_header()?;

        Ok(driver)
    }

    /// Flush the stored disk header.
    fn flush_header(&mut self) -> Result<(), disk::Error> {
        // Encode and write it to the disk.
        self.vdev.write(0, &self.header.encode())
    }
}

impl<D: Disk> Drop for Driver<D> {
    fn drop(&mut self) {
        // Set the state flag to close so we know that it was a proper shutdown.
        self.header.state_flag = StateFlag::Closed;
        // Flush the header.
        self.flush_header();
    }
}

impl<D: Disk> Disk for Driver<D> {
    fn number_of_sectors(&self) -> Sector {
        self.vdev.number_of_sectors()
    }

    fn write(sector: Sector, buf: &[u8]) -> Result<(), Error> {
        // Make sure it doesn't write to the null sector reserved for the disk header.
        assert_ne!(sector, 0, "Trying to write to the null sector.");

        // Forward the call to the inner vdev.
        self.vdev.write(sector, buf)
    }
    fn read(sector: Sector, buf: &mut [u8]) -> Result<(), Error> {
        // Make sure it doesn't write to the null sector reserved for the disk header.
        assert_ne!(sector, 0, "Trying to read from the null sector.");

        // Forward the call to the inner vdev.
        self.vdev.read(sector, buf)
    }
}

impl Vdev for Driver {
    fn heal(&mut self, sector: disk::Sector) -> Result<(), disk::Error> {
        // Forward the call to the inner vdev.
        self.vdev.read(sector)
    }
}
