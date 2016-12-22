//! Disk I/O
//!
//! This module provides primitives for disk I/O.
//!
//! We fix the sector size to 512, since it can be emulated by virtually any disk in use today.

/// A disk sector number.
type Sector = usize;

/// The logical sector size.
const SECTOR_SIZE: usize = 512;

/// A disk I/O error.
enum Error {
    /// The read or write exceeded the address space of the disk.
    ///
    /// This is triggered when the sector read or written to does not exist.
    OutOfBounds,
    /// The sector is determined to be corrupted per the hardware checks.
    ///
    /// Most modern hard disks implement some form of consistency checks. If said check fails, this
    /// error shall be returned.
    SectorCorrupted,
}

/// A storage device.
///
/// This trait acts similarly to `std::io::{Read, Write}`, but is designed specifically for disks.
trait Disk {
    /// The number of sectors on this disk.
    fn number_of_sectors(&self) -> Sector;

    /// Write data to the disk.
    ///
    /// This writes `buffer` into sector `sector`.
    fn write(sector: Sector, buffer: &[u8]) -> Result<(), Error>;
    /// Read data from the disk.
    ///
    /// This reads `buffer.len()` bytes into `buffer` from sector `sector`.
    fn read(sector: Sector, buffer: &mut [u8]) -> Result<(), Error>;
}

/// For testing, we allow byte slices to act as disks.
#[cfg(tests)]
impl Disk for &mut [u8] {
    fn number_of_sectors(&self) -> Sector {
        self.len() as Sector / SECTOR_SIZE
    }

    fn write(sector: Sector, buffer: &[u8]) -> Result<(), Error> {
        if sector as usize >= self.number_of_sectors() {
            Err(Error::OutOfBounds)
        } else {
            Ok(self[sector as usize / SECTOR_SIZE as usize..][..buffer.len()]
               .copy_from_slice(buffer))
        }
    }

    fn read(sector: Sector, buffer: &mut [u8]) -> Result<(), Error> {
        if sector as usize >= self.number_of_sectors() {
            Err(Error::OutOfBounds)
        } else {
            Ok(buffer.copy_from_slice(self[sector as usize / SECTOR_SIZE as usize..][..buffer.len()]))
        }
    }
}
