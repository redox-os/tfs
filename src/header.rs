const DISK_HEADER_SIZE: usize = 4096;
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

enum Error {
    CorruptConsistencyFlag,
    CorruptEncryptionAlgorithm,
    CorruptEncryptionParameters,
    CorruptImplementationId,
    CorruptStateBlockAddress,
    CorruptVersionNumber,
    IncompatibleVersion,
    UnknownCipher,
    UnknownConsistencyFlag,
    UnknownFormat,
    Disk(disk::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), Error> {
        write!(f, )
    }
}

enum MagicNumber {
    PartialCompatibility,
    TotalCompatibility,
}

enum EncryptionAlgorithm {
    Identity = 0,
    Speck128 = 1,
}

enum ConsistencyFlag {
    Closed,
    StillActive,
    Inconsistent,
    Uninitialized,
}

#[derive(Default)]
struct DiskHeader {
    magic_number: MagicNumber,
    version_number: u32,
    implementation_id: u32,
    encryption_algorithm: EncryptionAlgorithm,
    encryption_parameters: [u8; 16],
    state_block_address: ClusterPointer,
    consistency_flag: ConsistencyFlag,
}

impl DiskHeader {
    fn flush_encryption_algorithm<D: Disk>(disk: &mut D) -> Result<DiskHeader, Error> {
        
    }

    /// Load the disk header from some disk.
    ///
    /// This will construct it into memory while performing error checks on the header to ensure
    /// correctness.
    fn load<D: Disk>(disk: &mut D) -> Result<DiskHeader, Error> {
        // Load the disk header into a buffer in memory.
        let mut buf = [0; DISK_HEADER_SIZE];
        disk.read_all(0, 0, &mut buf)?;
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

        // == Encryption Section ==

        // Load the encryption algorithm choice.
        ret.encryption_algorithm = EncryptionAlgorithm::from(LittleEndian::read(buf[64..66]))?;
        // Repeat the bitwise negation.
        if ret.encryption_algorithm as u16 != !LittleEndian::read(buf[66..68]) {
            // The implementation ID is corrupt; abort.
            return Err(Error::CorruptEncryptionAlgorithm);
        }

        // Load the encryption parameters (e.g. salt).
        self.encryption_parameters.copy_from_slice(&buf[68..84]);
        // Repeat the bitwise negation.
        if self.encryption_parameters.iter().eq(buf[84..100].iter().map(|x| !x)) {
            // The encryption parameters are corrupt; abort.
            return Err(Error::CorruptEncryptionParameters);
        }

        // == State ==

        // Load the state block pointer.
        ret.state_block_address = ClusterPointer::new(LittleEndian::read(buf[128..136]));
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
