//! Disk header parsing.
//!
//! The disk header provides information on how to read a TFS disk. This module parses and
//! interprets the disk header so it is meaningful to the programmer.

/// The size of the disk header.
///
/// This should be a multiple of the cluster size.
const DISK_HEADER_SIZE: usize = 4096;
/// The current version number.
///
/// The versioning scheme divides this number into two parts. The 16 most significant bits identify
/// breaking changes. For two version A to be able to read an image written by version B, two
/// requirements must hold true:
///
/// 1. A must be greater than or equal to B.
/// 2. A and B must have equal higher parts.
const VERSION_NUMBER: u32 = 0;
/// The magic number of images with partial TFS compatibility.
const PARTIAL_COMPATIBILITY_MAGIC_NUMBER: &[u8] = b"~TFS fmt";
/// The magic number of images with total TFS compatibility.
const TOTAL_COMPATIBILITY_MAGIC_NUMBER: &[u8] = b"TFS fmt ";

quick_error! {
    /// A disk header reading error.
    enum Error {
        /// Unknown format (not TFS).
        UnknownFormat {
            description("Unknown format (not TFS).")
        }
        /// The version is incompatible with this implementation.
        ///
        /// The version number is given by some integer. If the higher half of the integer does not
        /// match, the versions are incompatible and this error is returned.
        IncompatibleVersion {
            description("Incompatible version.")
        }
        /// Unknown or implementation-specific checksum algorithm.
        UnknownChecksumAlgorithm {
            description("Unknown checksum algorithm option.")
        }
        /// Invalid checksum algorithm.
        InvalidChecksumAlgorithm {
            description("Invalid checksum algorithm option.")
        }
        /// Unknown state flag value.
        UnknownStateFlag {
            description("Unknown state flag.")
        }
        /// The checksums doesn't match.
        ChecksumMismatch {
            /// The checksum of the data.
            expected: u16,
            /// The expected/stored value of the checksum.
            found: u16,
        } {
            description("Mismatching checksum.")
            display("Mismatching checksums in the disk header - expected {:x}, found {:x}.", expected, found)
        }
        /// Non-existent vdev.
        InvalidVdev {
            /// The unknown label.
            label: u16,
        } {
            description("Invalid/nonexistent vdev in the vdev stack.")
            display("Invalid/nonexistent vdev with label {}.", label)
        }
        /// Unknown implementation defined vdev.
        UnknownVdev {
            description("Unknown, implementation defined vdev in the vdev stack.")
        }
    }
}

/// TFS magic number.
#[derive(PartialEq, Eq, Clone, Copy)]
enum MagicNumber {
    /// The image is partially compatible with the official TFS specification.
    PartialCompatibility,
    /// The image is completely compatible with the official TFS specification.
    TotalCompatibility,
}

impl TryFrom<&[u8]> for MagicNumber {
    type Err = ParseError;

    fn from(string: &[u8]) -> Result<MagicNumber, Error> {
        match string {
            // Partial compatibility.
            PARTIAL_COMPATIBILITY_MAGIC_NUMBER => Ok(MagicNumber::PartialCompatibility),
            // Total compatibility.
            TOTAL_COMPATIBILITY_MAGIC_NUMBER => Ok(MagicNumber::TotalCompatibility),
            // Unknown format; abort.
            _ => Err(Error::UnknownFormat),
        }
    }
}

impl Into<&'static [u8]> for MagicNumber {
    fn into(self) -> &[u8] {
        match self {
            MagicNumber::TotalCompatibility => TOTAL_COMPATIBILITY_MAGIC_NUMBER,
            MagicNumber::PartialCompatibility => PARTIAL_COMPATIBILITY_MAGIC_NUMBER,
        }
    }
}

/// A checksum algorithm configuration option.
enum ChecksumAlgorithm {
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
        match self {
            // Hash the thing via SeaHash, then take the 16 lowest bits (truncating cast).
            ChecksumAlgorithm::SeaHash => seahash::hash(buf),
        }
    }
}

impl TryFrom<u16> for ChecksumAlgorithm {
    type Err = Error;

    fn try_from(from: u16) -> Result<ChecksumAlgorithm, Error> {
        match from {
            1 => Ok(ChecksumAlgorithm::SeaHash),
            0x8000...0xFFFF => Err(Error::UnknownChecksumAlgorithm),
            _ => Err(Error::InvalidChecksumAlgorithm),
        }
    }
}

/// State flag.
///
/// The state flag defines the state of the disk, telling the user if it is in a consistent state
/// or not. It is important for doing non-trivial things like garbage-collection, where the disk
/// needs to enter an inconsistent state for a small period of time.
#[derive(PartialEq, Eq, Clone, Copy)]
enum StateFlag {
    /// The disk was properly closed and shut down.
    Closed = 0,
    /// The disk is active/was forcibly shut down.
    Open = 1,
    /// The disk is in an inconsistent state.
    ///
    /// Proceed with caution.
    Inconsistent = 2,
}

/// A virtual device.
///
/// Vdevs transforms one disk to another, in the sense that it changes the behavior of I/O
/// operations to give the disk some particular feature, such as error correction etc.
enum Vdev {
    /// A mirror.
    ///
    /// This mirrors the lower half of the disk to the higher half to provide ability to heal data.
    Mirror,
    /// SPECK encryption.
    ///
    /// This encrypts the disk with the SPECK cipher.
    Speck,
}

/// The disk header.
#[derive(Default, PartialEq, Eq, Clone, Copy)]
struct DiskHeader {
    /// The magic number.
    magic_number: MagicNumber,
    /// The version number.
    version_number: u32,
    /// An secret number randomly picked when initializing.
    uid: u128,
    /// The chosen checksum algorithm.
    checksum_algorithm: ChecksumAlgorithm,
    /// The state flag.
    state_flag: StateFlag,
    /// The vdev setup.
    ///
    /// A vdev is a "virtual device". Each entry in this field transforms one disk to another,
    /// effectively modifying the behavior of reads and writes. Each of the layers define another
    /// of such masks.
    ///
    /// Take this example of a vdev setup:
    ///
    ///     Mirror
    ///     Mirror
    ///     Encrypt
    ///
    /// What it means is that there are two mirrors, yielding 1:4 redundancy, and then encryption,
    /// which means that the data will be encrypted after mirrored.
    vdev_stack: Vec<Vdev>,
}

impl DiskHeader {
    /// Parse the disk header from some sequence of bytes.
    ///
    /// This will construct it into memory while performing error checks on the header to ensure
    /// correctness.
    fn decode(buf: &disk::SectorBuf) -> Result<DiskHeader, Error> {
        // # Introducer Section
        //
        // This section has the purpose of defining the implementation, version, and type of the
        // disk image. It is rarely changed unless updates or reformatting happens.

        // Load the magic number.
        let magic_number = MagicNumber::try_from(&buf[..8])?;

        // Load the version number.
        let version_number = LittleEndian::read(&buf[8..]);
        // Check if the version is compatible. If the higher half doesn't match, there were a
        // breaking change. Otherwise, if the version number is lower or equal to the current
        // version, it's compatible.
        if version_number >> 16 != VERSION_NUMBER >> 16 || version_number > VERSION_NUMBER {
            // The version is not compatible; abort.
            return Err(Error::IncompatibleVersion);
        }

        // # Unique identifier
        //
        // This section stores a single number, namely the UID. The UID is supposed to be a secret
        // ID used throughout the code, such as seed for hashing and salt for key stretching.
        let uid = LittleEndian::read(&buf[16..])

        // # Configuration
        //
        // This section stores certain configuration options needs to properly load the disk header.

        // Load the checksum algorithm config field.
        let checksum_algorithm = ChecksumAlgorithm::try_from(LittleEndian::read(buf[32..]))?;

        // # State section
        //
        // This section holds the state of disk and pointers to information on the state of the
        // file system.

        // Load the state flag.
        let state_flag = StateFlag::from(buf[48])?;

        // # Vdev setup
        //
        // This section holds information on how to read and write the disk, such as encryption and
        // redundancy.

        // The slice of the remaining vdev section.
        let mut vdev_section = &buf[64..504];
        // Generate the vdev stack.
        let mut vdev_stack = Vec::new();
        loop {
            // Check if there are more vdevs to read. The vdev section may only end in a
            // terminator, so there should be more.
            if vdev_section.len() < 2 {
                // There is no more labels and the terminator is not read yet. This is considered
                // an error.
                return Err(Error::MissingTerminatorVdev);
            }
            // Read the 16-bit label.
            let label = LittleEndian::read(vdev_section);
            // Cut off the two bytes of the label in the remaining slice (this won't ever panic due
            // to the `if` statement above).
            vdev_stack = &vdev_stack[2..];

            match label {
                // A terminator vdev was read; terminate, duh.
                0u16 => break,
                // A mirror vdev.
                1 => vdev_stack.push(Vdev::Mirror),
                // A SPECK encryption cipher.
                2 => vdev_stack.push(Vdev::Speck),
                // Implementation defined vdev, which this implementation does not support.
                0xFFFF => return Err(Error::UnknownVdev),
                // Invalid vdevs (vdevs that are necessarily invalid under this version).
                _ => return Err(Error::InvalidVdev {
                    label: label,
                }),
            }
        }

        // Make sure that the checksum of the disk header matches the 8 byte field in the end.
        let expected = LittleEndian::read(&buf[128..]);
        let found = ret.checksum_algorithm.hash(&buf[..128]);
        if expected != found {
            return Err(Error::ChecksumMismatch {
                expected: expected,
                found: found,
            });
        }

        DiskHeader {
            magic_number: magic_number,
            version_number: version_number,
            uid: uid,
            checksum_algorithm: checksum_algorithm,
            state_flag: state_flag,
            vdev_stack: vdev_stack,
        }
    }

    /// Encode the header into a sector-sized buffer.
    fn encode(&self) -> disk::SectorBuf {
        // Create a buffer to hold the data.
        let mut buf = [0; disk::SECTOR_SIZE];

        // Write the magic number.
        buf[..8].copy_from_slice(self.magic_number.into());

        // Write the current version number.
        LittleEndian::write(&mut buf[8..], VERSION_NUMBER);

        // Write the UID.
        LittleEndian::write(&mut buf[16..], self.uid);

        // Write the checksum algorithm.
        LittleEndian::write(&mut buf[32..], self.checksum_algorithm as u16);

        // Write the state flag.
        buf[48] = self.state_flag as u8;

        // Write the vdev stack.
        let mut vdev_section = &mut buf[64..504];
        for vdev in self.vdev_stack {
            match vdev {
                Vdev::Mirror => LittleEndian::write(vdev_section, 1u16),
                Vdev::Speck => LittleEndian::write(vdev_section, 2u16),
            }

            // Slide on.
            vdev_section = vdev_section[2..];
        }
        // Write the terminator vdev.
        vdev_section[0] = 0;
        vdev_section[1] = 0;

        // Calculate and write the checksum.
        LittleEndian::write(&mut buf[504..], self.checksum_algorithm.hash(&buf[..128]));

        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_identity() {
        let mut header = DiskHeader::default();
        assert_eq!(DiskHeader::decode(header.encode()).unwrap(), header);

        header.magic_number = MagicNumber::PartialCompatibility;
        assert_eq!(DiskHeader::decode(header.encode()).unwrap(), header);

        header.version_number = 1;
        assert_eq!(DiskHeader::decode(header.encode()).unwrap(), header);

        header.uid = 12;
        assert_eq!(DiskHeader::decode(header.encode()).unwrap(), header);

        header.state_flag = StateFlag::Inconsistent;
        assert_eq!(DiskHeader::decode(header.encode()).unwrap(), header);

        header.vdev_stack.push(Vdev::Speck {
            salt: 228309220937918,
        });
        assert_eq!(DiskHeader::decode(header.encode()).unwrap(), header);

        header.vdev_stack.push(Vdev::Mirror);
        assert_eq!(DiskHeader::decode(header.encode()).unwrap(), header);
    }

    #[test]
    fn manual_mutation() {
        let mut header = DiskHeader::default();
        let mut sector = header.encode();

        header.magic_number = MagicNumber::PartialCompatibility;
        sector[7] = b'~';

        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(sector, header.encode());

        header.version_number |= 0xFF;
        sector[8] = 0xFF;

        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(sector, header.encode());

        header.uid |= 0xFF;
        sector[16] = 0xFF;

        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(sector, header.encode());

        // TODO: This is currently somewhat irrelevant as there is only one cksum algorithm. When a
        //       second is added, change this to the non-default.
        header.checksum = ChecksumAlgorithm::SeaHash;
        sector[32] = 1;

        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(sector, header.encode());

        header.state_flag = StateFlag::Open;
        sector[48] = 1;

        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(sector, header.encode());

        header.vdev_stack.push(Vdev::Speck {
            salt: 0x7955,
        });
        sector[64] = 2;
        sector[66] = 0x55;
        sector[67] = 0x79;

        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(sector, header.encode());

        header.vdev_stack.push(Vdev::Mirror);
        sector[82] = 1;

        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(sector, header.encode());
    }

    #[test]
    fn unknown_format() {
        let mut sector = DiskHeader::default().encode();
        sector[0] = b'A';

        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(DiskHeader::decode(sector), Err(Error::UnknownFormat));
    }

    #[test]
    fn incompatible_version() {
        let mut sector = DiskHeader::default().encode();
        sector[11] = 0xFF;

        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(DiskHeader::decode(sector), Err(Error::IncompatibleVersion));
    }

    #[test]
    fn unknown_state_flag() {
        let mut sector = DiskHeader::default().encode();
        sector[48] = 6;
        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(DiskHeader::decode(sector), Err(Error::UnknownStateFlag));
    }

    #[test]
    fn wrong_checksum_algorithm() {
        let mut sector = DiskHeader::default().encode();

        sector[32] = 0;
        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(DiskHeader::decode(sector), Err(Error::InvalidChecksumAlgorithm));
        sector[33] = 0x80;
        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(DiskHeader::decode(sector), Err(Error::UnknownChecksumAlgorithm));
    }

    #[test]
    fn wrong_vdev() {
        let mut sector = DiskHeader::default().encode();
        sector[64] = 0xFF;
        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(DiskHeader::decode(sector), Err(Error::InvalidVdev));
        sector[65] = 0xFF;
        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(DiskHeader::decode(sector), Err(Error::UnknownVdev));

        sector = DiskHeader::default().encode();
        sector[64] = 1;
        sector[66] = 0xFF;
        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(DiskHeader::decode(sector), Err(Error::InvalidVdev));
        sector[67] = 0xFF;
        LittleEndian::write(&mut sector[504..], seahash::hash(sector[..504]));
        assert_eq!(DiskHeader::decode(sector), Err(Error::UnknownVdev));
    }

    #[test]
    fn checksum_mismatch() {
        let mut sector = DiskHeader::default().encode();

        sector[5] = 28;
        assert_eq!(DiskHeader::decode(sector), Err(Error::ChecksumMismatch));

        sector = DiskHeader::default().encode();

        sector[500] = 28;
        assert_eq!(DiskHeader::decode(sector), Err(Error::ChecksumMismatch));
    }
}
