//! Virtual devices.
//!
//! A virtual device or "vdev" is a disk with some extra capabilities. It transforms operations to
//! other operationss in order to provide new features.
//!
//! Vdevs themself can be seen as an image (transformation) of another disk. They might modify the
//! sector enumeration or provide some redundancy, encryption, or similar features working on disk
//! level.
//!
//! The term vdev has similar meaning in the context of ZFS.
//!
//! It is important that vdevs keep the invariants of the inner vdev. In particular, it may not
//! leave to an inconsistent state, unless the inner vdev does.

use std::mem;
use futures::{future, Future};

use Error;
use disk::{self, Disk};
use disk::header::{self, DiskHeader};

/// A driver transforming a normal disk into a disk respecting the vdev setup.
///
/// It reads the vdev setup from the disk header, which it fetches from the disk. Then it builds
/// the vdev stack, which it stores.
///
/// Importantly, this subtracts the disk header, so sector `0` is really sector `1` of the inner
/// disk.
pub struct Driver<D> {
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
    disk: D,
}

impl<D: Disk> Driver<D> {
    /// Set up the driver from some disk.
    ///
    /// This will load the disk header from `disk` and construct the driver. It will also set the
    /// disk to be in open state. If any encryption is enabled, `password` will be used as the
    /// password.
    ///
    /// The result is wrapped in a future, which represents the operation, such that it can be
    /// executed asynchronously.
    fn open<D: Disk>(disk: D, password: &[u8]) -> future!(Driver<D>) {
        info!(disk, "loading the state and initializing the driver");

        // Read the disk header.
        debug!(disk, "read the disk header");
        disk.read(0).and_then(|header| {
            let driver = Driver {
                header: DiskHeader::decode(header)?,
                disk: disk,
            };

            match driver.header.state_flag {
                // Throw a warning if it wasn't properly shut down.
                header::StateFlag::Open => {
                    warn!(driver, "the disk's state flag is still open, likely wasn't properly shut \
                                   down last time; beware of data loss");
                },
                // The state inconsistent; throw an error.
                header::StateFlag::Inconsistent => return Err(err!(Corruption, "the file system is in an inconsistent state, possibly due to crash")),
            }

            // Set the state flag to open.
            debug!(driver, "setting the state flag to 'open'");
            driver.header.state_flag = header::StateFlag::Open;

            // Update the version.
            debug!(driver, "updating the version number";
                   "old version" => header.version_number,
                   "new version" => header::VERSION_NUMBER);
            driver.header.version_number = header::VERSION_NUMBER;

            Ok(driver)
        }).and_then(|driver| {
            // Flush the updated header.
            driver.flush_header().map(|_| driver)
        })
    }

    /// Initialize a disk with a new header.
    ///
    /// This sets the disk header (provided by the `header` argument) of disk `disk` and returns
    /// the driver representing the disk.
    ///
    /// It is used as an entry point to create a new file system.
    fn init<D: Disk>(disk: D, options: header::Options) -> future!(Driver<D>) {
        info!(disk, "creating a new system");

        // Create the new header from the user-specified options.
        let header = DiskHeader::new(options);
        // Write the header to the disk.
        disk.write(0, header.encode()).map(|_| Driver {
            header: header,
            disk: disk,
        })
    }

    /// Flush the stored disk header.
    ///
    /// This returns a future, which carries this operation. First when the future has completed,
    /// the operations has been executed.
    fn flush_header(&self) -> future!(()) {
        debug!(self, "flushing the disk header");

        // Encode and write it to the disk.
        self.disk.write(0, &self.header.encode())
    }
}

impl<D: Disk> Drop for Driver<D> {
    fn drop(&mut self) {
        info!(self, "closing the driver");

        // Set the state flag to close so we know that it was a proper shutdown.
        debug!(self, "setting state flag to 'closed'");
        self.header.state_flag = header::StateFlag::Closed;
        // Flush the header.
        self.flush_header().wait().unwrap();
    }
}

delegate_log!(Driver.disk);

impl<D: Disk> Disk for Driver<D> {
    type ReadFuture  = D::ReadFuture;
    type WriteFuture = D::WriteFuture;
    type TrimFuture  = D::TrimFuture;

    fn number_of_sectors(&self) -> disk::Sector {
        // Start out with the raw number of sectors. We subtract one to cut of the disk header.
        let mut sectors = self.disk.number_of_sectors() - 1;

        // Go over the vdev stack.
        for vdev in self.header.vdev_stack {
            match vdev {
                // Mirrors divide the disk in half, as the higher half must mirror the lower.
                header::Vdev::Mirror => sectors /= 2,
                header::Vdev::Speck => (),
            }
        }
    }

    fn read(&self, sector: disk::Sector) -> D::ReadFuture {
        // We start out by reading the inner buffer. We subtract one to cut of the disk header.
        let mut buf = self.disk.read(sector + 1);

        // Go over the vdev stack.
        for vdev in self.header.vdev_stack {
            // Note that it is very important that `sector` gets updated to account for changed
            // address space.

            match vdev {
                // TODO
                header::Vdev::Speck => unimplemented!(),
                _ => (),
            }
        }
    }

    fn write(&self, sector: disk::Sector, buf: &disk::SectorBuf) -> D::WriteFuture {
        // Start a vector to hold the writes. This allows us to rewrite the write operations for
        // every vdev transformation.
        let mut writes = vec![(sector, buf)];

        // Go over the vdev stack.
        for vdev in self.header.vdev_stack {
            match vdev {
                // Mirror the higher and lower half.
                header::Vdev::Mirror => for i in 0..writes.len() {
                    // Write the higher half.
                    writes.push((writes[i].0 * 2, writes[i].1));
                },
                // TODO
                header::Vdev::Speck => unimplemented!(),
            }
        }

        // Execute all the writes, we've buffered.
        future::join_all(writes.into_iter().map(|(sector, buf)| {
            self.disk.write(sector, buf)
        }))
    }

    fn trim(&self, sector: disk::Sector) -> D::TrimFuture {
        // Start a vector to track what sectors to trim.
        let mut trims = vec![sector];

        // Go over the vdev stack.
        for vdev in self.header.vdev_stack {
            match vdev {
                // Mirror the higher and lower half.
                header::Vdev::Mirror => for i in 0..writes.len() {
                    // Trim the higher half's sector.
                    trims.push(trims[i]);
                },
                // Encryption doesn't matter for trimming.
                header::Vdev::Speck => (),
            }
        }

        // Execute all the trims, we've buffered.
        future::join_all(trims.into_iter().map(|sector| {
            self.disk.trim(sector)
        }))
    }
}
