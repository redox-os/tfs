const SCRYPT_LOG_N: u8 = 20;
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;

enum Cipher {
    Identity,
    Speck128 {
        key: u128,
    },
}

struct Encrypted<D> {
    inner: D,
    cipher: Cipher,
}

impl<D: Disk> Encrypted<D> {
    pub fn new(disk: D, password: &[u8], header: &header::DiskHeader) -> Encrypted<D> {
        match header.cipher {
            header::Cipher::Identity => Encrypted {
                inner: disk,
                cipher: Identity,
            },
            header::Cipher::Speck128 => {
                // Use scrypt to generate the key from the password and salt.
                let mut key = [0; 16];
                scrypt::scrypt(password, seed, &scrypt::ScryptParams::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P), &mut key);

                Encrypted {
                    inner: disk,
                    cipher: Speck128 {
                        key: LittleEndian::read(key),
                    },
                }
            },
        }
    }
}

impl<D: Disk> Disk for Encrypted<D> {
    fn sector_size(&self) -> SectorOffset {
        match self.cipher {
            &Cipher::Identity => self.inner.sector_size(),
            _ => unimplemented!(),
        }
    }
    fn number_of_sectors(&self) -> Sector {
        match self.cipher {
            &Cipher::Identity => self.inner.number_of_sectors(),
            _ => unimplemented!(),
        }
    }

    fn write(sector: Sector, offset: SectorOffset, buffer: &[u8]) -> Result<(), Error> {
        match self.cipher {
            &Cipher::Identity => self.inner.write(sector, offset, buffer),
            _ => unimplemented!(),
        }
    }
    fn read(sector: Sector, offset: SectorOffset, buffer: &mut [u8]) -> Result<(), Error> {
        match self.cipher {
            &Cipher::Identity => self.inner.read(sector, offset, buffer),
            _ => unimplemented!(),
        }
    }
}
