/// A disk sector number.
type Sector = u64;
/// An offset into a sector (in bytes).
type SectorOffset = u16;

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
    /// The size (in bytes) of a disk sector.
    ///
    /// This might vary across disks, but TFS requires it to be at least 128 bytes.
    fn sector_size(&self) -> SectorOffset;
    /// The number of sectors on this disk.
    fn number_of_sectors(&self) -> Sector;

    /// Write data to the disk.
    ///
    /// This writes `buffer` into sector `sector` starting at `offset` bytes into the sector.
    fn write(sector: Sector, offset: SectorOffset, buffer: &[u8]) -> Result<(), Error>;
    /// Read data from the disk.
    ///
    /// This reads `buffer.len()` bytes into `buffer` from sector `sector` starting at `offset`
    /// bytes.
    fn read(sector: Sector, offset: SectorOffset, buffer: &mut [u8]) -> Result<(), Error>;
}

/// For testing, we allow byte slices to act as disks.
#[cfg(tests)]
impl Disk for &mut [u8] {
    fn sector_size(&self) -> SectorOffset {
        512
    }

    fn number_of_sectors(&self) -> Sector {
        self.len() as Sector / self.sector_size()
    }

    fn write(sector: Sector, offset: SectorOffset, buffer: &[u8]) -> Result<(), Error> {
        if offset + buffer.len() > self.sector_size() {
            Err(Error::OutOfBounds)
        } else {
            Ok(self[(sector / self.sector_size() + offset as Sector) as usize..][..buffer.len()]
               .copy_from_slice(buffer))
        }
    }

    fn read(sector: Sector, offset: SectorOffset, buffer: &mut [u8]) -> Result<(), Error> {
        if offset + buffer.len() > self.sector_size() {
            Err(Error::OutOfBounds)
        } else {
            Ok(buffer.copy_from_slice(self[(sector / self.sector_size() + offset as Sector) as usize..][..buffer.len()]))
        }
    }
}
