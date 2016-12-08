const SCRYPT_LOG_N: u8 = 20;
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;

struct Speck128 {
    key: u128,
}

struct Identity;

struct Encrypted<D, C> {
    inner: D,
    header: header::DiskHeader,
    cipher: C,
}

impl<D: Disk> Encrypted<D, Speck128> {
    pub fn new(disk: D, password: &[u8]) -> Encrypted<D> {
        // Read the disk header.
        let header = header::DiskHeader::load(disk);

        // Use scrypt to generate the key from the password and salt.
        let mut key = [0; 16];
        scrypt::scrypt(password, header.encryption_parameters, &scrypt::ScryptParams::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P), &mut key);

        Encrypted {
            inner: disk,
            header: header,
            cipher: Speck128 {
                key: LittleEndian::read(key),
            },
        }
    }
}

impl<D: Disk> Encrypted<D, Identity> {
    pub fn new(disk: D) -> Encrypted<D> {
        // Read the disk header.
        let header = header::DiskHeader::load(disk);

        Encrypted {
            inner: disk,
            header: header,
            cipher: Identity,
        }
    }
}

impl<D: Disk> Disk for Encrypted<D, Identity> {
    fn sector_size(&self) -> SectorOffset {
        self.inner.sector_size()
    }
    fn number_of_sectors(&self) -> Sector {
        self.inner.number_of_sectors()
    }

    fn write(sector: Sector, offset: SectorOffset, buffer: &[u8]) -> Result<(), Error> {
        self.inner.write(sector, offset, buffer)
    }
    fn read(sector: Sector, offset: SectorOffset, buffer: &mut [u8]) -> Result<(), Error> {
        self.inner.read(sector, offset, buffer)
    }
}
