enum Error {
    WrongPassword,
    UnknownChecksumAlgorithm,
    UnknownCompressionAlgorithm,
    InvalidChecksum,
    Disk(disk::Error),
}

enum ChecksumAlgorithm {
    Constant,
    SeaHash,
}

enum CompressionAlgorithm {
    Identity,
    Lz4,
}

struct StateBlock {
    checksum_algorithm: ChecksumAlgorithm,
    compression_algorithm: CompressionAlgorithm,
    freelist_head: clusters::Pointer,
    super_page: clusters::Pointer,
}

impl StateBlock {
    fn new(buf: &[u8]) -> Result<(), Error> {
        if &buf[..4] != &[0, 0, 0, 0] {
            return Err(Error::WrongPassword);
        }

        let checksum_algorithm = match LittleEndian::read(buf[16..18]) {
            0 => ChecksumAlgorithm::Constant,
            1 => ChecksumAlgorithm::SeaHash,
            _ => return Err(Error::UnknownChecksumAlgorithm),
        };

        if checksum_algorithm.hash(&buf[..40]) != LittleEndian::read(&buf[40..44]) {
            return Err(Error::InvalidChecksum);
        }

        StateBlock {
            checksum_algorithm: checksum_algorithm,
            compression_algorithm: match LittleEndian::read(buf[18..20]) {
                0 => CompressionAlgorithm::Identity,
                1 => CompressionAlgorithm::Lz4,
                _ => return Err(Error::UnknownCompressionAlgorithm),
            },
            freelist_head: LittleEndian::read(buf[32..40]),
            super_page: LittleEndian::read(buf[40..48]),
        }

    }
}
