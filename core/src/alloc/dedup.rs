//! Data deduplication.
//!
//! This module provides data structures for eliminating duplicates at a page level, meaning that
//! if two equal pages are allocated, they can be reduced to one, reducing the space used.

use crossbeam::sync::AtomicOption;
use ring::digest;
use std::sync::atomic;

use {little_endian, disk};
use alloc::page;

/// The atomic ordering used in the table.
const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;

/// A SHA-256 fingerprint of a page.
///
/// It is broken into two `u128` since `u256` isn't supported yet.
// TODO: ^^^^
struct Fingerprint(u128, u128);

impl Fingerprint {
    /// Fingerprint a page.
    ///
    /// This calculates the fingerprint of page `buf` through SHA-2.
    fn new(buf: &disk::SectorBuf) -> Fingerprint {
        // Hash it into a 256-bit value.
        let hash = digest::digest(digest::SHA256, buf).as_ref();

        // Read it in two parts to get two `u128`s.
        (little_endian::read(hash), little_endian::read(hash[16..]))
    }
}

/// The maximal number of pagess the table can contain.
const MAX_PAGES_IN_TABLE: usize = 1 << 16;

/// A deduplication candidate.
///
/// This is a potential match. It stores data to check if it is a complete match.
#[derive(Copy, Clone)]
struct Candidate {
    /// The candidate for deduplication.
    ///
    /// This is a page pointer of some page which is potentially containing the same data, as the
    /// page we're allocating. If it is indeed a match, it is sufficient to use this page instead
    /// of allocating a new.
    page: page::Pointer,
    /// The fingerprint of the page data.
    ///
    /// No fingerprint function mapping a domain to a smaller codomain is injective (gives unique
    /// fingerprints), but with wide enough fingerprints, finding collisions gets practically
    /// impossible. Even if an user had malicious intends, they cannot compute a collision.
    fingerprint: Fingerprint,
}

impl Candidate {
    /// Check if this candidate matches some data buffer.
    ///
    /// If not, `false` is returned.
    fn is_match(&self, buf: &disk::SectorBuf) -> bool {
        // Check the fingerprint against the hash of the buffer. Again, this is strictly speak
        // heuristic, but for all practical purposes, no collisions will ever be found.
        self.fingerprint == Fingerprint::new(buf)
    }
}

/// A deduplication table.
///
/// Deduplication tables stores information needed to determine if some page already exist or the
/// disk or not. They're heuristic in the sense that sometimes a duplicate may exists but not be
/// deduplicated. This is due to the fact that there is no probing and thus checksum collisions
/// cannot be resolved. Therefore, it will replace a random old candidate.
#[derive(Default)]
pub struct Table {
    /// The table of candidates.
    ///
    /// When looking up a particular candidate, the checksum modulo the table size is used. If this
    /// entry is `None`, there is no candidate.
    table: [AtomicOption<Candidate>; MAX_PAGES_IN_TABLE],
}

impl Table {
    /// Find a duplicate of some page.
    ///
    /// This searches for a duplicate of `buf` which has checksum `cksum`. If no duplicate is
    /// found, `None` is returned.
    fn dedup(&self, buf: &disk::SectorBuf, cksum: u32) -> Option<page::Pointer> {
        // We look up in the table with the checksum under some modulus, since that is faster to
        // calculate than a cryptographic hash, meaning that we can refine candidates based on a
        // rougher first-hand measure.
        let entry = self.table[cksum % MAX_PAGES_IN_TABLE];

        // Temporarily remove the entry from the table.
        if let Some(candidate) = entry.take(ORDERING) {
            // A candidate exists.

            // Put it back into the entry.
            entry.swap(candidate);

            // Check if the checksum and fingerprint matches.
            if cksum == candidate.page.checksum && candidate.isMatch(buf) {
                // Yup.
                Some(candidate.page)
            } else {
                // Nup.
                None
            }
        } else {
            // No candidate was stored in the table.
            None
        }
    }

    /// Insert a page into the table.
    ///
    /// This inserts page `page` with data `buf` into the deduplication table.
    fn insert(&mut self, buf: &disk::SectorBuf, page: page::Pointer) {
        // Overwrite the old entry with the new updated entry.
        self.table[page.checksum % MAX_PAGES_IN_TABLE].swap(Candidate {
            page: page,
            // TODO: This fingerprint might be double-calculated due to the use in `dedup`.
            fingerprint: Fingerprint::new(buf),
        }, ORDERING);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate() {
        let mut table = Table::default();
        let p1 = page::Pointer {
            cksum: 7,
            .. Default::default()
        };
        let p2 = page::Pointer {
            cksum: 13,
            .. Default::default()
        };

        table.insert(&Default::default(), p1);
        table.insert(&Default::default(), p2);

        assert_eq!(table.dedup(&Default::default(), 7), p1);
        assert_eq!(table.dedup(&Default::default(), 13), p2);
    }

    #[test]
    fn checksum_collision() {
        let mut table = Table::default();
        let p1 = page::Pointer {
            cksum: 7,
            .. Default::default()
        };
        let p2 = page::Pointer {
            cksum: 7,
            cluster: cluster::Pointer::new(100).unwrap(),
            .. Default::default()
        };

        table.insert([0; disk::SECTOR_SIZE], p1);
        table.insert([1; disk::SECTOR_SIZE], p2);

        assert_eq!(table.dedup(&Default::default(), 7), p2);
    }
}
