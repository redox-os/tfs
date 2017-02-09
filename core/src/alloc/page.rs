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

/// A page pointer.
///
/// Page pointer contains information necessary for read and write pages on the disk. They're
/// similar to clutter pointer in that sense, but they contain more information:
///
/// 1. The cluster the page is stored in.
/// 2. _How_ to read the page from the cluster.
/// 3. A checksum of the page.
struct Pointer {
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

impl Into<u128> for Pointer {
    fn into(self) -> u128 {
        // Shift and OR to set up the integer as described in the specification.
        self.cluster as u128
            | (self.offset.map_or(!0, |offset| {
                // TODO: Consider removing this.
                assert_ne!(offset, !0, "The page offset cannot be 0xFFFFFFFF, as it collides with \
                           the serialization of `PageOffset::Uncompressed`.");

                offset
            }) as u128) << 64
            | (self.checksum as u128) << 64 << 32
    }
}

impl From<u128> for Pointer {
    fn from(from: u128) -> Pointer {
        Pointer {
            // The 64 lowest bits are used for the cluster.
            cluster: from as u64,
            // Next the page offset is stored.
            offset: match (from >> 64) as u32 {
                // Again, the trap value !0 represents an uncompressed cluster.
                0xFFFFFFFF => None,
                // This cluster was compressed and the offset is `n`.
                n => Some(n),
            },
            // The highest 32 bit then store the checksum.
            checksum: (from >> 64 >> 32) as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_inverse(x: u128) {
        assert_eq!(Pointer::from(x).into(), x);
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
        let mut ptr = Pointer::from(0x0101010101010101FEFFFFFFCCCCCCCC);

        assert_eq!(ptr.cluster, 0x0101010101010101);
        assert_eq!(ptr.offset, Some(!0 - 1));
        assert_eq!(ptr.checksum, 0xCCCCCCCC);

        ptr = Pointer::from(0x0101010101010101FFFFFFFFCCCCCCCC);

        assert_eq!(ptr.cluster, 0x0101010101010101);
        assert_eq!(ptr.offset, None);
        assert_eq!(ptr.checksum, 0xCCCCCCCC);

    }
}
