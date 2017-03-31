use std::convert::TryFrom;
use {little_endian, Error};
use alloc::page;
use disk::{self, cluster};

/// A compression algorithm configuration option.
pub enum CompressionAlgorithm {
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
            0x8000...0xFFFF => Err(err!(Corruption, "unknown implementation-defined compression algorithm option {:x}", from)),
            _ => Err(err!(Corruption, "invalid compression algorithm option {:x}", from)),
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
    ///
    /// This cluster contains pointers to other free clusters. If not full, it is padded with
    /// zeros.
    cluster: cluster::Pointer,
    /// The checksum of the freelist head up to the last free cluster.
    ///
    /// This is the checksum of the metacluster (at `self.cluster`).
    checksum: u64,
}

/// The state sub-block.
pub struct State {
    /// A pointer to the superpage.
    pub superpage: Option<page::Pointer>,
    /// The freelist head.
    ///
    /// If the freelist is empty, this is set to `None`.
    pub freelist_head: Option<FreelistHead>,
}

/// The options sub-block.
pub struct Options {
    /// The chosen compression algorithm.
    pub compression_algorithm: CompressionAlgorithm,
}

/// The TFS state block.
pub struct StateBlock {
    /// The static options section of the state block.
    pub options: Options,
    /// The dynamic state section of the state block.
    pub state: State,
}

impl StateBlock {
    /// Parse the binary representation of a state block.
    fn decode(
        buf: &disk::SectorBuf,
        checksum_algorithm: disk::header::ChecksumAlgorithm,
    ) -> Result<StateBlock, Error> {
        // Make sure that the checksum of the state block matches the 8 byte field in the start.
        let expected = little_endian::read(&buf);
        let found = checksum_algorithm.hash(&buf[8..]);
        if expected != found {
            return Err(err!(Corruption, "mismatching checksums in the state block - expected \
                            {:x}, found {:x}", expected, found));
        }

        Ok(StateBlock {
            options: Options {
                // Load the compression algorithm config field.
                compression_algorithm: CompressionAlgorithm::try_from(little_endian::read(buf[8..]))?,
            },
            state: State {
                // Load the superpage pointer.
                superpage: little_endian::read(buf[16..]),
                // Construct the freelist head metadata. If the pointer is 0, we return `None`.
                freelist_head: little_endian::read(&buf[32..]).map(|freelist_head| {
                    FreelistHead {
                        cluster: freelist_head,
                        // Load the checksum of the freelist head.
                        checksum: little_endian::read(&buf[40..]),
                    }
                }),
            },
        })
    }

    /// Encode the state block into a sector-sized buffer.
    fn encode(&self, checksum_algorithm: disk::header::ChecksumAlgorithm) -> disk::SectorBuf {
        // Create a buffer to hold the data.
        let mut buf = disk::SectorBuf::default();

        // Write the compression algorithm.
        little_endian::write(&mut buf[8..], self.options.compression_algorithm as u16);
        // Write the superpage pointer. If no superpage is initialized, we simply write a null
        // pointer.
        little_endian::write(&mut buf[16..], self.state.superpage);

        if let Some(freelist_head) = self.state.freelist_head {
            // Write the freelist head pointer.
            little_endian::write(&mut buf[32..], freelist_head.cluster);
            // Write the checksum of the freelist head.
            little_endian::write(&mut buf[40..], freelist_head.checksum);
        }
        // If the free list was empty, both the checksum, and pointer are zero, which matching the
        // buffer's current state.

        // Calculate and store the checksum.
        let cksum = checksum_algorithm.hash(&buf[8..]);
        little_endian::write(&mut buf, cksum);

        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use error;

    #[test]
    fn inverse_identity() {
        let mut block = StateBlock::default();
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);

        block.options.compression_algorithm = CompressionAlgorithm::Identity;
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);

        block.state.superpage = 200;
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);

        block.state.freelist_head = Some(FreelistHead {
            cluster: 22,
            checksum: 2,
        });
        assert_eq!(StateBlock::decode(block.encode()).unwrap(), block);
    }

    #[test]
    fn manual_mutation() {
        let mut block = StateBlock::default();
        let mut sector = block.encode();

        block.options.compression_algorithm = CompressionAlgorithm::Identity;
        sector[9] = 0;
        little_endian::write(&mut sector, seahash::hash(sector[8..]));
        assert_eq!(sector, block.encode());

        block.state.superpage = 29;
        sector[16] = 29;
        little_endian::write(&mut sector, seahash::hash(sector[8..]));
        assert_eq!(sector, block.encode());

        block.state.freelist_head = Some(FreelistHead {
            cluster: 22,
            checksum: 2,
        });
        sector[32] = 22;
        sector[40] = 2;
        little_endian::write(&mut sector, seahash::hash(sector[8..]));
        assert_eq!(sector, block.encode());
    }

    #[test]
    fn mismatching_checksum() {
        let mut sector = StateBlock::default().encode();
        sector[2] = 20;
        assert_eq!(StateBlock::decode(sector).unwrap_err().kind, error::Kind::Corruption);
    }

    #[test]
    fn unknown_invalid_options() {
        let mut sector = StateBlock::default().encode();

        sector = StateBlock::default().encode();

        sector[8] = 0xFF;
        assert_eq!(StateBlock::decode(sector).unwrap_err().kind, error::Kind::Corruption);
    }
}
