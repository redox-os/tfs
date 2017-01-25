quick_error! {
    /// A state block parsing error.
    enum Error {
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
            0x8000...0xFFFF => Err(Error::UnknownCompressionAlgorithm),
            _ => Err(Error::InvalidCompressionAlgorithm),
        }
    }
}

/// The freelist head.
///
/// The freelist chains some number of blocks containing pointers to free blocks. This allows for
/// simple and efficient allocation. This struct stores information about the head block in the
/// freelist.
struct FreelistHead {
    /// A pointer to the head of the freelist.
    cluster: cluster::Pointer,
    /// The checksum of the freelist head up to the last free cluster.
    ///
    /// This is the checksum from the start of the metacluster (at `self.cluster`) to the last free
    /// cluster.
    ///
    /// The beautiful thing is that we can update this without passing through an inconsistent
    /// state as we can simultaneously increment or decrement `self.counter`, since the state block
    /// is stored on only one sector.
    checksum: u64,
    /// The number of free clusters in the freelist head.
    ///
    /// The reason for having this counter as opposed to simply zeroing the used pointers is that
    /// with this counter, we don't have to worry about inconsistency as the checksum can be
    /// updated together with the counter, and furthermore, we don't need to write the
    /// metacluster's sector, improving performance.
    counter: u8,
}

/// The TFS state block.
struct StateBlock {
    /// The chosen compression algorithm.
    compression_algorithm: CompressionAlgorithm,
    /// A pointer to the superpage.
    superpage: Option<page::Pointer>,
    /// The freelist head.
    ///
    /// If the freelist is empty, this is set to `None`.
    freelist_head: Option<FreelistHead>,
}

impl StateBlock {
    /// Parse the binary representation of a state block.
    fn decode(buf: &disk::SectorBuf, checksum_algorithm: header::ChecksumAlgorithm) -> Result<StateBlock, Error> {
        // Make sure that the checksum of the state block matches the 8 byte field in the start.
        let expected = LittleEndian::read(&buf);
        let found = checksum_algorithm.hash(&buf[8..]);
        if expected != found {
            return Err(Error::ChecksumMismatch {
                expected: expected,
                found: found,
            });
        }

        Ok(StateBlock {
            // Load the compression algorithm config field.
            compression_algorithm: CompressionAlgorithm::try_from(LittleEndian::read(buf[8..]))?,
            // Load the superpage pointer.
            superpage: page::Pointer::new(LittleEndian::read(buf[16..])),
            // Construct the freelist head metadata. If the pointer is 0, we return `None`.
            freelist_head: cluster::Pointer::new(LittleEndian::read(&buf[32..])).map(|freelist_head| {
                FreelistHead {
                    cluster: freelist_head,
                    // Load the checksum of the freelist head.
                    checksum: LittleEndian::read(&buf[40..]),
                    // Load the pointer counter in the freelist head.
                    counter: buf[48],
                }
            }),
        })
    }

    /// Encode the state block into a sector-sized buffer.
    fn encode(&self, checksum_algorithm: header::ChecksumAlgorithm) -> disk::SectorBuf {
        // Create a buffer to hold the data.
        let mut buf = [0; disk::SECTOR_SIZE];

        // Write the compression algorithm.
        LittleEndian::write(&mut buf[8..], self.compression_algorithm as u16);
        // Write the superpage pointer. If no superpage is initialized, we simply write a null
        // pointer.
        LittleEndian::write(&mut buf[16..], self.superpage.map_or(0, |x| x.into()));

        if let Some(freelist_head) = self.freelist_head {
            // Write the freelist head pointer.
            LittleEndian::write(&mut buf[32..], freelist_head.cluster);
            // Write the checksum of the freelist head.
            LittleEndian::write(&mut buf[40..], freelist_head.checksum);
            // Write the freelist head counter.
            buf[48] = freelist_head.counter;
        }
        // If the free list was empty, both the checksum, counter, and pointer are zero, which
        // matching the buffer's current state.

        // Calculate and store the checksum.
        let cksum = self.checksum_algorithm.hash(&buf[8..]);
        LittleEndian::write(&mut buf, cksum);

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

        block.compression_algorithm = CompressionAlgorithm::Identity;
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);

        block.superpage = 200;
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);

        block.freelist_head = Some(FreelistHead {
            cluster: 22,
            checksum: 2,
            counter: 2,
        });
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);
    }

    #[test]
    fn manual_mutation() {
        let mut block = StateBlock::default();
        let mut sector = block.encode();

        block.compression_algorithm = CompressionAlgorithm::Identity;
        sector[9] = 0;
        LittleEndian::write(&mut sector, seahash::hash(sector[8..]));
        assert_eq!(sector, block.encode());

        block.superpage = 29;
        sector[16] = 29;
        LittleEndian::write(&mut sector, seahash::hash(sector[8..]));
        assert_eq!(sector, block.encode());

        block.freelist_head = Some(FreelistHead {
            cluster: 22,
            checksum: 2,
            counter: 3,
        });
        sector[32] = 22;
        sector[40] = 2;
        sector[48] = 3;
        LittleEndian::write(&mut sector, seahash::hash(sector[8..]));
        assert_eq!(sector, block.encode());
    }

    #[test]
    fn mismatching_checksum() {
        let mut sector = StateBlock::default().encode();
        sector[2] = 20;
        assert_eq!(StateBlock::decode(sector), Err(Error::ChecksumMismatch));
    }

    #[test]
    fn unknown_invalid_options() {
        let mut sector = StateBlock::default().encode();

        sector = StateBlock::default().encode();

        sector[8] = 0xFF;
        assert_eq!(StateBlock::decode(sector), Err(Error::InvalidCompression));
    }
}
