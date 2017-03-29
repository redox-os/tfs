mod cache;
pub mod cluster;
mod crypto;
mod header;
mod vdev;

use futures::Future;
use {slog, Error};

/// The logical sector size.
pub const SECTOR_SIZE: usize = 512;
/// The size of a sector pointer.
pub const SECTOR_POINTER_SIZE: usize = 8;

/// A disk sector number.
pub type Sector = usize;
/// A buffer of sector size.
pub type SectorBuf = [u8; SECTOR_SIZE];

/// A storage device.
///
/// This trait acts similarly to `std::io::{Read, Write}`, but is designed specifically for disks.
pub trait Disk: slog::Drain {
    /// The future returned from read operations.
    ///
    /// In order to avoid performance hit of copying a whole sector around, we allocate the data on
    /// the heap through `Box<T>`.
    type ReadFuture: Future<Item = Box<SectorBuf>, Error = Error>;
    /// The future returned from write operations.
    type WriteFuture: Future<Item = (), Error = Error>;

    /// The number of sectors on this disk.
    fn number_of_sectors(&self) -> Sector;
    /// Read data from the disk directly into the return value.
    ///
    /// The result is wrapped in a future, which represents the operation, such that it can be
    /// done asynchronously.
    fn read(&self, sector: Sector) -> Self::ReadFuture;
    /// Write data to the disk.
    ///
    /// This returns a future, which carries the operation writing `buf` into sector `sector`.
    /// First when the future has completed, the operation has been executed.
    fn write(&self, sector: Sector, buf: &SectorBuf) -> Self::WriteFuture;
}
