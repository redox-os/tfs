//! Pages.
//!
//! Pages are like virtual clusters: they share the same size and can contain the same data, but
//! they're represented specially on the disk. The distinction between pages and clusters is what
//! allows for random-access full-disk compression.
//!
//! Clusters can contain at least one page, but often more: this is achieved by concatenating and
//! compressing the pages. If they can be compressed enough to fit into a single cluster, the
//! compressed data will be stored in the cluster. If not, the cluster can contain the page
//! uncompressed, in which pages and clusters coincide.
//!
//! To distinguish between various pages in a cluster, the pointer contains offset information.
//! There is a reserved offset value for representing uncompressed clusters as well.
//!
//! In other words, RACC (the name of the algorithm) greedily tries to fit as many pages into a
//! cluster by compressing the pages together. To avoid storing metadata in the clusters, the
//! pointers contains this information instead.

use little_endian;
use disk::cluster;

/// The size (in bytes) of a serialized page pointer.
pub const POINTER_SIZE: usize = 16;

/// A page pointer.
///
/// Page pointer contains information necessary for read and write pages on the disk. They're
/// similar to clutter pointer in that sense, but they contain more information:
///
/// 1. The cluster the page is stored in.
/// 2. _How_ to read the page from the cluster.
/// 3. A checksum of the page.
pub struct Pointer {
    /// The cluster in which the page is stored.
    cluster: cluster::Pointer,
    /// The offset into the decompressed stream.
    ///
    /// Clusters can be either uncompressed (containing one page) or compressed (containing some
    /// number of pages). This field contains information about _how_ to load the page, namely if
    /// the cluster is compressed or not, and if compressed, what the offset to read it from the
    /// decompressed stream.
    ///
    /// If this is `None`, the page can be read directly from the cluster without any
    /// decompression.
    ///
    /// If this is `Some(offset)`, the cluster must be decompressed and the page can be read
    /// `offset` pages into the decompressed stream. `offset` is assumed to never be `!0` in order
    /// to ensure the serialization to be injective.
    offset: Option<u32>,
    /// Checksum of the page.
    ///
    /// This checksum is calculated through the algorithm specified in the disk header, and when
    /// the page is read, it is compared against the page's expected checksum to detect possible
    /// data corruption.
    ///
    /// The reason for storing this in the pointer as opposed to in the cluster is somewhat
    /// complex: It has multiple benefits. For one, we avoid resizing the clusters so they match
    /// the standard sector size, but more importantly, we avoid the [self-validation
    /// problem](https://blogs.oracle.com/bonwick/entry/zfs_end_to_end_data). Namely, it is able to
    /// detect phantom writes.
    ///
    /// The idea was originally conceived by Bonwick (main author of ZFS), who thought that the
    /// file system could be organized like a Merkle tree of checksums.
    ///
    /// Most other approaches have the issue of not detecting phantom writes or not preserving
    /// consistency on crashes.
    checksum: u32,
}

impl little_endian::Encode for Pointer {
    fn write_le(self, into: &mut [u8]) {
        // The lowest bytes are dedicated to the cluster pointer.
        little_endian::write(into, self.cluster);
        // Next, we write the page offset, which is needed for knowing where the pointer points to
        // in the decompressed stream.
        little_endian::write(&mut into[cluster::POINTER_SIZE..], if let Some(offset) = self.offset {
            // TODO: Consider removing this.
            assert_ne!(offset, !0, "The page offset cannot be 0xFFFFFFFF, as it collides with \
                       the serialization of the uncompressed page offset.");

            offset
        } else {
            // When there is no offset, we use `!0` to represent that it is uncompressed.
            !0
        });
        // Lastly, we write the checksum.
        little_endian::write(&mut into[cluster::POINTER_SIZE..][32..], self.checksum);
    }
}

impl little_endian::Decode for Option<Pointer> {
    fn read_le(from: &[u8]) -> Option<Pointer> {
        // The 64 lowest bits are used for the cluster.
        little_endian::read(from).map(|cluster| Pointer {
            cluster: cluster,
            // Next the page offset is stored.
            offset: match little_endian::read(&from[cluster::POINTER_SIZE..]) {
                // Again, the trap value !0 represents an uncompressed cluster.
                0xFFFFFFFF => None,
                // This cluster was compressed and the offset is `n`.
                n => Some(n),
            },
            // The highest 32 bit then store the checksum.
            checksum: little_endian::read(&from[cluster::POINTER_SIZE..][32..]),
        })
    }
}

impl little_endian::Encode for Option<Pointer> {
    fn write_le(self, into: &mut [u8]) {
        if let Some(ptr) = self {
            // Simply write the inner pointer into the buffer.
            little_endian::write(into, self)
        } else {
            // Zero the first `POINTER_SIZE` bytes of the buffer (null pointer).
            for i in &mut into[..POINTER_SIZE] {
                *i = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_inverse(x: u128) {
        let mut buf = [0; 16];
        little_endian::write(&mut buf, x);
        assert_eq!(little_endian::read(&buf), x);
    }

    #[test]
    fn inverse_identity() {
        assert_inverse(38);
        assert_inverse(0x0101010101010101FEFFFFFF21231234);
        assert_inverse(0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF);

        // Randomized testing.
        for mut x in 0u128..1000000 {
            // I'm in fucking love with this permutation.
            x = x.wrapping_mul(0x6eed0e9da4d94a4f6eed0e9da4d94a4f);
            x ^= (x >> 64) >> (x >> 120);
            x = x.wrapping_mul(0x6eed0e9da4d94a4f6eed0e9da4d94a4f);

            assert_inverse(x)
        }
    }

    #[test]
    fn fixed_values() {
        let mut ptr = Pointer::from(&[0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0xFE, 0xFF,
                                      0xFF, 0xFF, 0xCC, 0xCC, 0xCC, 0xCC]);

        assert_eq!(ptr.cluster, 0x0101010101010101);
        assert_eq!(ptr.offset, Some(!0 - 1));
        assert_eq!(ptr.checksum, 0xCCCCCCCC);

        ptr = Pointer::from(&[0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0xFF, 0xFF, 0xFF,
                              0xFF, 0xCC, 0xCC, 0xCC, 0xCC]);

        assert_eq!(ptr.cluster, 0x0101010101010101);
        assert_eq!(ptr.offset, None);
        assert_eq!(ptr.checksum, 0xCCCCCCCC);

    }
}
