type Sector = u64;
type SectorOffset = u16;

enum Error {
    OutOfBounds,
    SectorCorrupted,
}

trait Disk {
    fn sector_size(&self) -> SectorOffset;
    fn number_of_sectors(&self) -> Sector;

    fn write(sector: Sector, offset: SectorOffset, buffer: &[u8]) -> Result<(), Error>;
    fn read(sector: Sector, offset: SectorOffset, buffer: &mut [u8]) -> Result<(), Error>;

    fn write_all(sector: Sector, offset: SectorOffset, buffer: &[u8]) -> Result<(), Error> {

    }
    fn read_all(sector: Sector, offset: SectorOffset, buffer: &mut [u8]) -> Result<(), Error> {
        let sector_size = self.sector_size();

        self.read(buffer.len() / sector_size, offset, buffer[..offset])?;
        for i in 1..buffer.len() as Sector / sector_size + 1 {
            self.read(sector + i, 0, buffer[i * sector_size..])
        }
    }
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
