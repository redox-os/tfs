quick_error! {
    /// A state block parsing error.
    enum Error {
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
        ChecksumMismatch {
            /// The checksum of the data.
            expected: u16,
            /// The expected/stored value of the checksum.
            found: u16,
        } {
            display("Mismatching checksums in the state block - expected {:x}, found {:x}.", expected, found)
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

impl ChecksumAlgorithm {
    /// Produce the checksum of the buffer through the algorithm.
    pub fn hash(self, buf: &[u8]) -> u64 {
        // The behavior depends on the chosen checksum algorithm.
        match self.state.state_block.checksum {
            // Constant checksums. These are a bit weird, but in the end it makes sense: a number
            // is fixed to some value (in this case, the highest 16-bit integer), under the
            // assumption that if a sector is damaged, all of it is affected, hence the number
            // shouldn't match. Obviously, this isn't true for every disk, therefore one must
            // be careful before using this.
            ChecksumAlgorithm::Constant => !0,
            // Hash the thing via SeaHash, then take the 16 lowest bits (truncating cast).
            ChecksumAlgorithm::SeaHash => seahash::hash(buf),
        }
    }
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
    fn decode(buf: &[u8]) -> Result<(), Error> {
        // Load the checksum algorithm config field.
        let checksum_algorithm = ChecksumAlgorithm::try_from(LittleEndian::read(buf[0..2]))?;

        // Make sure that the checksum of the state block matches the 4 byte field in the start.
        let expected = LittleEndian::read(&buf[64..70]);
        let found = checksum_algorithm.hash(&buf[..64]);
        if expected != found {
            return Err(Error::ChecksumMismatch {
                expected: expected,
                found: found,
            });
        }

        StateBlock {
            checksum_algorithm: checksum_algorithm,
            // Load the compression algorithm config field.
            compression_algorithm: CompressionAlgorithm::try_from(LittleEndian::read(buf[2..4]))?,
            // Load the freelist head pointer.
            freelist_head: LittleEndian::read(buf[32..40]),
            // Load the superpage pointer.
            superpage: LittleEndian::read(buf[40..48]),
        }
    }

    /// Encode the state block into a sector-sized buffer.
    fn encode(&self) -> [u8; disk::SECTOR_SIZE] {
        // Create a buffer to hold the data.
        let mut buf = [0; disk::SECTOR_SIZE];

        // Write the checksum algorithm.
        LittleEndian::write(&mut buf[0..], self.checksum_algorithm as u16);
        // Write the compression algorithm.
        LittleEndian::write(&mut buf[2..], self.compression_algorithm as u16);
        // Write the freelist head pointer.
        LittleEndian::write(&mut buf[32..], self.freelist_head);
        // Write the superpage pointer.
        LittleEndian::write(&mut buf[40..], self.superpage);

        // Calculate and store the checksum.
        let cksum = self.checksum_algorithm.hash(&buf[..64]);
        LittleEndian::write(&mut buf[64..], cksum);

        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_identity() {
        let mut block = StateBlock::default();
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);

        block.checksum_algorithm = ChecksumAlgorithm::Constant;
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);

        block.compression_algorithm = CompressionAlgorithm::Identity;
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);

        block.freelist_head = 2000;
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);

        block.superpage = 200;
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);
    }

    #[test]
    fn manual_mutation() {
        let mut block = StateBlock::default();
        let mut sector = block.encode();

        block.checksum_algorithm = ChecksumAlgorithm::Constant;
        sector[0] = 0;
        LittleEndian::write(&mut sector[64..], 0xFFFFFFFFFFFFFFFF);

        assert_eq!(sector, block.encode());

        block.compression_algorithm = CompressionAlgorithm::Identity;
        sector[2] = 0;

        assert_eq!(sector, block.encode());

        block.freelist_head = 52;
        sector[32] = 52;

        assert_eq!(sector, block.encode());

        block.superpage = 29;
        sector[40] = 29;

        assert_eq!(sector, block.encode());
    }

    #[test]
    fn mismatching_checksum() {
        let mut sector = StateBlock::default().encode();
        sector[2] = 20;
        if let Err(Error::ChecksumMismatch { .. }) = StateBlock::decode(sector) {} else {
            panic!();
        }
    }

    #[test]
    fn unknown_invalid_options() {
        let mut sector = StateBlock::default().encode();

        sector[0] = 0xFF;
        assert_eq!(StateBlock::decode(sector), Err(Error::InvalidChecksumAlgorithm));
        sector[1] = 0xFF;
        assert_eq!(StateBlock::decode(sector), Err(Error::UnknownChecksumAlgorithm));

        sector = StateBlock::default().encode();

        sector[2] = 0xFF;
        assert_eq!(StateBlock::decode(sector), Err(Error::InvalidCompression));
        sector[3] = 0xFF;
        assert_eq!(StateBlock::decode(sector), Err(Error::UnknownChecksumAlgorithm));
    }
}
