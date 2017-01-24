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
    /// Read data from the disk.
    ///
    /// This reads sector `sector` into buffer `buf`.
    fn read(&self, sector: Sector, buf: &mut SectorBuf) -> Result<(), Error>;
    /// Heal a sector.
    ///
    /// This heals sector `sector`, through the provided redundancy, if possible.
    ///
    /// Note that after it is called, it is still necessary to check if the healed sector is valid,
    /// as there is a certain probability that the recovery will fail.
    fn heal(&mut self, sector: disk::Sector) -> Result<(), disk::Error>;
}

/// For testing, we allow byte slices to act as disks.
#[cfg(tests)]
impl Disk for &mut [u8] {
    fn number_of_sectors(&self) -> Sector {
        self.len() as Sector / SECTOR_SIZE
    }

    fn write(sector: Sector, buffer: SectorBuf) -> Result<(), Error> {
        // Check if the sector is within bounds.
        if sector as usize >= self.number_of_sectors() {
            Err(Error::OutOfBounds)
        } else {
            Ok(self[sector as usize / SECTOR_SIZE as usize..][..disk::SECTO ]
               .copy_from_slice(buf))
        }
    }

    fn read(sector: Sector, buf: &mut SectorBuf) -> Result<(), Error> {
        // Check if the sector is within bounds.
        if sector as usize >= self.number_of_sectors() {
            Err(Error::OutOfBounds)
        } else {
            Ok(buf.copy_from_slice(self[sector as usize / SECTOR_SIZE as usize..][..disk::SECTOR_SIZE]))
        }
    }

    fn heal(&mut self, sector: disk::Sector) -> Result<(), disk::Error> {
        Err(Error::HealFailed {
            sector: sector,
        })
    }
}
