quick_error! {
    /// A state block parsing error.
    enum Error {
        /// Wrong password or corrupt state block.
        WrongPassword {
            description("Invalid password or corrupt salt.")
        }
        /// Unknown or implementation-specific checksum algorithm.
        UnknownChecksumAlgorithm {
            description("Unknown checksum algorithm option.")
        }
        /// Invalid checksum algorithm.
        InvalidChecksumAlgorithm {
            description("Invalid checksum algorithm option.")
        }
        /// Unknown or implementation-specific compression algorithm.
        UnknownCompressionAlgorithm {
            description("Unknown compression algorithm option.")
        }
        /// Invalid compression algorithm.
        InvalidCompressionAlgorithm {
            description("Invalid compression algorithm option.")
        }
        /// The checksums doesn't match.
        InvalidChecksum {
            description("Mismatching checksum.")
        }
    }
}

/// A checksum algorithm configuration option.
enum ChecksumAlgorithm {
    /// Constant checksums.
    ///
    /// This is entirely independent of the checksummed data.
    Constant = 0,
    /// SeaHash checksum.
    ///
    /// SeaHash was designed for TFS, and is described [in this
    /// post](http://ticki.github.io/blog/seahash-explained/).
    SeaHash = 1,
}

impl TryFrom<u16> for ChecksumAlgorithm {
    type Err = Error;

    fn try_from(from: u16) -> Result<ChecksumAlgorithm, Error> {
        match from {
            0 => Ok(ChecksumAlgorithm::Constant),
            1 => Ok(ChecksumAlgorithm::SeaHash),
            1 << 15... => Err(Error::UnknownChecksumAlgorithm),
            _ => Err(Error::InvalidChecksumAlgorithm),
        }
    }
}

/// A compression algorithm configuration option.
enum CompressionAlgorithm {
    /// Identity function/compression disabled.
    Identity = 0,
    /// LZ4 compression.
    ///
    /// LZ4 is a very fast LZ77-family compression algorithm. Like other LZ77 compressors, it is
    /// based on streaming data reduplication. The details are described
    /// [here](http://ticki.github.io/blog/how-lz4-works/).
    Lz4 = 1,
}

impl TryFrom<u16> for CompressionAlgorithm {
    type Err = Error;

    fn try_from(from: u16) -> Result<CompressionAlgorithm, Error> {
        match from {
            0 => Ok(CompressionAlgorithm::Identity),
            1 => Ok(CompressionAlgorithm::Lz4),
            1 << 15... => Err(Error::UnknownCompressionAlgorithm),
            _ => Err(Error::InvalidCompressionAlgorithm),
        }
    }
}

/// The TFS state block.
struct StateBlock {
    /// The chosen checksum algorithm.
    checksum_algorithm: ChecksumAlgorithm,
    /// The chosen compression algorithm.
    compression_algorithm: CompressionAlgorithm,
    /// A pointer to the head of the freelist.
    freelist_head: cluster::Pointer,
    /// A pointer to the superpage.
    superpage: pages::Pointer,
}

impl StateBlock {
    /// Parse a sequence of bytes.
    fn parse(buf: &[u8]) -> Result<(), Error> {
        // The zeros field is used for making sure that the decryption was successful. If it fails,
        // the field should be uniformly distributed, and thus 2^-32 probability that the field is
        // wrongly decrypted to zeros. However, if the password is correct, it should decrypt to
        // the zeros. This is a simple form of MAC.
        if &buf[..4] != &[0, 0, 0, 0] {
            return Err(Error::WrongPassword);
        }

        // Load the checksum algorithm config field.
        let checksum_algorithm = ChecksumAlgorithm::try_from(LittleEndian::read(buf[16..18]))?;

        // Make sure that the checksum of the state block matches the 4 byte number following it.
        if checksum_algorithm.hash(&buf[..40]) != LittleEndian::read(&buf[40..44]) {
            return Err(Error::InvalidChecksum);
        }

        StateBlock {
            checksum_algorithm: checksum_algorithm,
            // Load the compression algorithm config field.
            compression_algorithm: CompressionAlgorithm::try_from(LittleEndian::read(buf[18..20]))?,
            // Load the freelist head pointer.
            freelist_head: LittleEndian::read(buf[32..40]),
            // Load the superpage pointer.
            superpage: LittleEndian::read(buf[40..48]),
        }
    }
}
