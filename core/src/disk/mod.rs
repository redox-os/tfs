mod cache;
mod crypto;
mod vdev;
pub mod cluster;
pub mod header;

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

/// A cached disk with a TFS header.
pub type TfsDisk<D> = cache::Cached<vdev::Driver<D>>;

/// Load the TFS disk.
///
/// This does not initialize or create the structure. It will merely load the disk.
pub fn open<D: Disk>(disk: D, password: &[u8]) -> future!(TfsDisk<D>) {
    vdev::Driver::open(disk).cached()
}

/// Initialize/create the TFS disk.
///
/// This creates the structure (given some options given in `options`) of the disk, and effectively
/// initializes a system.
pub fn init<D: Disk>(disk: D, options: header::Options) -> future!(TfsDisk<D>) {
    vdev::Driver::init(disk, options).cached()
}

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
    /// The future returned from the trim operations.
    type TrimFuture: Future<Item = (), Error = Error>;

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
    /// Inform the disk that a sector is no longer in use.
    ///
    /// This returns a future, which carries the operation trimming sector `sector`. First when the
    /// future has completed, the operation has been executed.
    fn trim(&self, sector: Sector) -> Self::TrimFuture;

    /// Create a cached version of the disk.
    fn cached(self) -> cache::Cached<Self> {
        cache::Cached::new(self)
    }
}
