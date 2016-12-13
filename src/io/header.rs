//! Disk header parsing.
//!
//! The disk header provides information on how to read a TFS disk. This module parses and
//! interprets the disk header so it is meaningful to the programmer.

/// The size of the disk header.
///
/// This should be a multiple of the cluster size.
const DISK_HEADER_SIZE: usize = 4096;
/// The default disk header.
const DEFAULT_DISK_HEADER: &'static [u8] = &[
    // The magic number (`TFS fmt `).
    b'T', b'F', b'S', b' ', b'f', b'm', b't', b' ',
    // The version number.
    0x00, 0x00, 0x00, 0x00,
    0xFF, 0xFF, 0xFF, 0xFF,
    // The implementation ID (`official`).
     b'o',  b'f',  b'f',  b'i',  b'c',  b'i',  b'a',  b'l',
    !b'o', !b'f', !b'f', !b'i', !b'c', !b'i', !b'a', !b'l',
    // Encryption algorithm.
    0x00, 0x00,
    0xFF, 0xFF,
    // Encryption parameters.
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    // State block address (uninitialized).
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // Consistency flag.
    0x03, 0xFC,
];

/// A disk header reading error.
enum Error {
    /// The consistency flag is corrupt.
    CorruptConsistencyFlag,
    /// The cipher field is corrupt.
    CorruptCipher,
    /// The encryption parameters is corrupt.
    CorruptEncryptionParameters,
    /// The implementation ID is corrupt.
    CorruptImplementationId,
    /// The state block address is corrupt.
    CorruptStateBlockAddress,
    /// The version number is corrupt.
    CorruptVersionNumber,
    /// The version is incompatible with this implementation.
    ///
    /// The version number is given by some integer. If the higher half of the integer does not
    /// match, the versions are incompatible and this error is returned.
    IncompatibleVersion,
    /// Unknown cipher option.
    UnknownCipher,
    /// Unknown consistency flag value.
    UnknownConsistencyFlag,
    /// Unknown format (not TFS).
    UnknownFormat,
    /// A disk I/O error.
    Disk(disk::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), Error> {
        write!(f, )
    }
}

/// TFS magic number.
enum MagicNumber {
    /// The image is partially compatible with the official TFS specification.
    PartialCompatibility,
    /// The image is completely compatible with the official TFS specification.
    TotalCompatibility,
}

/// Cipher option.
enum Cipher {
    /// Disk encryption disabled.
    Identity = 0,
    /// Use the SPECK cipher.
    Speck128 = 1,
}

/// Consistency flag.
///
/// The consistency flag defines the state of the disk, telling the user if it is in a consistent
/// state or not. It is important for doing non-trivial things like garbage-collection, where the
/// disk needs to enter an inconsistent state for a small period of time.
enum ConsistencyFlag {
    /// The disk was properly closed and shut down.
    Closed,
    /// The disk is active/was forcibly shut down.
    StillActive,
    /// The disk is in an inconsistent state.
    ///
    /// Proceed with caution.
    Inconsistent,
    /// The disk is uninitialized.
    Uninitialized,
}

/// The disk header.
#[derive(Default)]
struct DiskHeader {
    /// The magic number.
    magic_number: MagicNumber,
    /// The version number.
    version_number: u32,
    /// The implementation ID.
    implementation_id: u32,
    /// The cipher.
    cipher: Cipher,
    /// The encryption paramters.
    ///
    /// These are used as defined by the choice of cipher. Some ciphers might use it for salt or
    /// settings, and others not use it at all.
    encryption_parameters: [u8; 16],
    /// The address of the state block.
    state_block_address: clusters::Pointer,
    /// The consistency flag.
    consistency_flag: ConsistencyFlag,
}

impl DiskHeader {
    /// Load the disk header from some disk.
    ///
    /// This will construct it into memory while performing error checks on the header to ensure
    /// correctness.
    fn load<D: Disk>(disk: &mut D) -> Result<DiskHeader, Error> {
        // Load the disk header into a buffer in memory.
        let mut buf = [0; DISK_HEADER_SIZE];
        // Fetch the sector size.
        let sector_size = disk.sector_size();
        // Load the first couple of sectors into `buf`.
        for sector in 0..DISK_HEADER_SIZE / sector_size {
            disk.read(sector, 0, &mut buf[sector * sector_size..])?;
        }

        // Start with some default value, which will be filled out later.
        let mut ret = DiskHeader::default();

        // # Introducer Section
        //
        // This section has the purpose of defining the implementation, version, and type of the
        // disk image. It is rarely changed unless updates or reformatting happens.

        // Load the magic number.
        ret.magic_number = match buf[..8] {
            // Total compatibility.
            b"TFS fmt " => MagicNumber::TotalCompatibility,
            // Partial compatibility.
            b"~TFS fmt" => MagicNumber::PartialCompatibility,
            // Unknown format; abort.
            _ => return Err(Error::UnknownFormat),
        };

        // Load the version number.
        ret.version_number = LittleEndian::read(buf[8..12]);
        // Right after the version number, the same number follows, but bitwise negated. Make sure
        // that these numbers match (if it is bitwise negated). The reason for using this form of
        // code rather than just repeating it as-is is that if one overwrites all bytes with a
        // constant value, like zero, it won't be detected.
        if ret.version_number == !LittleEndian::read(buf[12..16]) {
            // Check if the version is compatible.
            if ret.version_number >> 16 > 0 {
                // The version is not compatible; abort.
                return Err(Error::IncompatibleVersion);
            }
        } else {
            // The version number is corrupt; abort.
            return Err(Error::CorruptVersionNumber);
        }

        // Load the implementation ID.
        ret.implementation_id = LittleEndian::read(buf[16..24]);
        // Similarly to the version number, a bitwise negated repetition follows. Make sure it
        // matches.
        if ret.implementation_id != !LittleEndian::read(buf[24..32]) {
            // The implementation ID is corrupt; abort.
            return Err(Error::CorruptImplementationId);
        }

        //////////////// Encryption Section ////////////////

        // Load the encryption algorithm choice.
        ret.cipher = Cipher::from(LittleEndian::read(buf[64..66]))?;
        // Repeat the bitwise negation.
        if ret.cipher as u16 != !LittleEndian::read(buf[66..68]) {
            // The implementation ID is corrupt; abort.
            return Err(Error::CorruptCipher);
        }

        // Load the encryption parameters (e.g. salt).
        self.encryption_parameters.copy_from_slice(&buf[68..84]);
        // Repeat the bitwise negation.
        if self.encryption_parameters.iter().eq(buf[84..100].iter().map(|x| !x)) {
            // The encryption parameters are corrupt; abort.
            return Err(Error::CorruptEncryptionParameters);
        }

        //////////////// State ////////////////

        // Load the state block pointer.
        ret.state_block_address = clusters::Pointer::new(LittleEndian::read(buf[128..136]));
        // Repeat the bitwise negation.
        if ret.state_block_address as u64 != !LittleEndian::read(buf[136..144]) {
            // The state block address is corrupt; abort.
            return Err(Error::CorruptStateBlockAddress);
        }

        // Load the consistency flag.
        self.consistency_flag = ConsistencyFlag::from(buf[144])?;
        // Repeat the bitwise negation.
        if self.consistency_flag as u8 != !buf[145] {
            // The consistency flag is corrupt; abort.
            return Err(Error::CorruptConsistencyFlag);
        }
    }
}
