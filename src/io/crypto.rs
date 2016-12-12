//! Cryptography.

/// A cipher.
///
/// This represents the user's choice of cipher to encrypt the disk.
enum Cipher {
    /// Identity/no encryption.
    Identity,
    /// SPECK-128 in XEX mode with scrypt keystretching.
    Speck128 {
        /// The derived key.
        ///
        /// This key is derived from the salt (given in the encryption paramters section of the
        /// disk header) and a password, by the scrypt algorithm with some hardcoded parameters.
        key: u128,
    },
}

/// An decrypted virtual disk.
///
/// This structure gaps the raw encrypted disk to an decrypted storage device that the software can
/// read from.
struct Decrypted<D> {
    /// The inner, encrypted disk.
    inner: D,
    /// The chosen cipher.
    ///
    /// This is given in the disk header.
    cipher: Cipher,
}

impl<D: Disk> Decrypted<D> {
    /// Construct a decrypted virtual disk from an encrypted disk and a password.
    ///
    /// The header is read in advance to allow the caller to access it properly.
    pub fn new(disk: D, password: &[u8], header: &header::DiskHeader) -> Encrypted<D> {
        match header.cipher {
            // The user has chosen not to encrypt his or her disk. Sad!
            header::Cipher::Identity => Encrypted {
                inner: disk,
                cipher: Identity,
            },
            /// The user is very wise and has chosen to encrypt the disk.
            header::Cipher::Speck128 => {
                /// The `log n` parameter for scrypt.
                const SCRYPT_LOG_N: u8 = 20;
                /// The `r` parameter for scrypt.
                const SCRYPT_R: u32 = 8;
                /// The `p` parameter for scrypt.
                const SCRYPT_P: u32 = 1;

                // Use scrypt to generate the key from the password and salt.
                let mut key = [0; 16];
                scrypt::scrypt(password, seed, &scrypt::ScryptParams::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P), &mut key);

                Encrypted {
                    inner: disk,
                    cipher: Speck128 {
                        // Read the scrypt-generated pad into a single integer, used as the key for
                        // our cipher.
                        key: LittleEndian::read(key),
                    },
                }
            },
        }
    }
}

impl<D: Disk> Disk for Decrypted<D> {
    fn sector_size(&self) -> SectorOffset {
        match self.cipher {
            // Encryption disabled; forward the call to the inner disk.
            &Cipher::Identity => self.inner.sector_size(),
            _ => unimplemented!(),
        }
    }
    fn number_of_sectors(&self) -> Sector {
        match self.cipher {
            // Encryption disabled; forward the call to the inner disk.
            &Cipher::Identity => self.inner.number_of_sectors(),
            _ => unimplemented!(),
        }
    }

    fn write(sector: Sector, offset: SectorOffset, buffer: &[u8]) -> Result<(), Error> {
        match self.cipher {
            // Encryption disabled; forward the call to the inner disk.
            &Cipher::Identity => self.inner.write(sector, offset, buffer),
            _ => unimplemented!(),
        }
    }
    fn read(sector: Sector, offset: SectorOffset, buffer: &mut [u8]) -> Result<(), Error> {
        match self.cipher {
            // Encryption disabled; forward the call to the inner disk.
            &Cipher::Identity => self.inner.read(sector, offset, buffer),
            _ => unimplemented!(),
        }
    }
}
