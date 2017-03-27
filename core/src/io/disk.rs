//! Disk I/O
//!
//! This module provides primitives for disk I/O.
//!
//! We fix the sector size to 512, since it can be emulated by virtually any disk in use today.

/// A disk sector number.
type Sector = usize;
/// A buffer of sector size.
type SectorBuf = [u8; disk::SECTOR_SIZE];

/// The logical sector size.
const SECTOR_SIZE: usize = 512;
/// The size of a sector pointer.
const SECTOR_POINTER_SIZE: usize = 8;

/// A storage device.
///
/// This trait acts similarly to `std::io::{Read, Write}`, but is designed specifically for disks.
trait Disk: slog::Drain {
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
