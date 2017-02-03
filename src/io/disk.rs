//! Disk I/O
//!
//! This module provides primitives for disk I/O.
//!
//! We fix the sector size to 512, since it can be emulated by virtually any disk in use today.

/// A disk sector number.
type Sector = usize;

#[derive(Default)]
type SectorBuf = [u8; disk::SECTOR_SIZE];

/// The logical sector size.
const SECTOR_SIZE: usize = 512;
/// The size of a sector pointer.
const SECTOR_POINTER_SIZE: usize = 8;

quick_error! {
    /// A disk I/O error.
    enum Error {
        /// The read or write exceeded the address space of the disk.
        ///
        /// This is triggered when the sector read or written to does not exist.
        OutOfBounds {
            /// The sector out-of-bounds.
            sector: Sector,
        } {
            display("Disk sector {} past end of disk.", sector)
            description("Disk sector past end of disk.")
        }
        /// The sector is determined to be corrupted per the hardware checks.
        ///
        /// Most modern hard disks implement some form of consistency checks. If said check fails, this
        /// error shall be returned.
        CorruptSector {
            /// The corrupt sector.
            sector: Sector,
        } {
            display("Disk sector {} is corrupt.", sector)
            description("Corrupt disk sector.")
        }
    }
}

/// A storage device.
///
/// This trait acts similarly to `std::io::{Read, Write}`, but is designed specifically for disks.
trait Disk {
    /// The number of sectors on this disk.
    fn number_of_sectors(&self) -> Sector;

    /// Write data to the disk.
    ///
    /// This writes buffer `buf` into sector `sector`.
    fn write(&mut self, sector: Sector, buf: &SectorBuf) -> Result<(), Error>;

    /// Read data from the disk into some buffer.
    ///
    /// This reads sector `sector` into buffer `buf`.
    fn read_to(&self, sector: Sector, buf: &mut SectorBuf) -> Result<(), Error>;
    /// Read data from the disk directly into the return value.
    #[inline]
    fn read(&self, sector: Sector) -> Result<SectorBuf, Error> {
        // Make a temporary buffer.
        let mut buf = SectorBuf::default();
        // Read the data into the buffer.
        self.read_to(sector, &mut buf)?;

        Ok(buf)
    }

    /// Heal a sector.
    ///
    /// This heals sector `sector`, through the provided redundancy, if possible.
    ///
    /// Note that after it is called, it is still necessary to check if the healed sector is valid,
    /// as there is a certain probability that the recovery will fail.
    fn heal(&mut self, sector: disk::Sector) -> Result<(), disk::Error>;
}
