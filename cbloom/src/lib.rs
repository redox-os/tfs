//! A concurrent implementation of Bloom filters.
//!
//! Bloom filters is a simple data structure, which is used in many different situations. It can
//! neatly solve certain problems heaurustically without need for extreme memory usage.
//!
//! This implementation is fairly standard, except that it uses atomic integers to work
//! concurrently.

#![feature(integer_atomics)]

use std::cmp;
use std::sync::atomic::{self, AtomicU64};

/// The atomic ordering used throughout the crate.
const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;

/// Hash an integer.
///
/// This is a pseudorandom permutation of `u64` with high statistical quality. It can thus be used
/// as a hash function.
fn hash(mut x: u64) -> u64 {
    // The following is copied from SeaHash.

    x = x.wrapping_mul(0x6eed0e9da4d94a4f);
    let a = x >> 32;
    let b = x >> 60;
    x ^= a >> b;
    x = x.wrapping_mul(0x6eed0e9da4d94a4f);

    // We XOR with some constant to make it zero-sensitive.
    x ^ 0x11c92f7574d3e84f
}

/// A concurrent Bloom filter.
///
/// Bloom filters are a probabilistic data structure, which allows you to insert elements, and
/// later test if they were inserted. The filter will either know it doesn't contain the element,
/// or that it might. It will never be "sure", hence the name "filter".
///
/// It works by having an array of bits. Every element is hashed into a sequence of these bits. The
/// bits of the inserted elements are set to 1. When testing for membership, we simply AND the
/// bits.
pub struct Filter {
    /// The bit array.
    ///
    /// We use `u64` to improve performance of `Filter::clear()`.
    bits: Vec<AtomicU64>,
    /// The number of hash functions.
    hashers: usize,
}

impl Filter {
    /// Get the chunk of a particular hash.
    #[inline]
    fn get(&self, hash: u64) -> &AtomicU64 {
        &self.bits[(hash as usize / 64) % self.bits.len()]
    }

    /// Create a new Bloom filter with the optimal number of hash functions.
    ///
    /// This creates a Bloom filter with `bytes` bytes of internal data, and optimal number (for
    /// `expected_elements` number of elements) of hash functions.
    pub fn new(bytes: usize, expected_elements: usize) -> Filter {
        // The number of hashers are calculated by multiplying the bits per element by ln(2), which
        // we approximate through multiplying by an integer, then shifting. To make things more
        // precise, we add 0x8000 to round the shift.
        Filter::with_size_and_hashers(bytes, (bytes / expected_elements * 45426 + 0x8000) >> 16)
    }

    /// Create a new Bloom filter with some number of bytes and hashers.
    ///
    /// This creates a Bloom filter with at least `bytes` bytes of internal data and `hashers`
    /// number of hash functions.
    ///
    /// If `hashers` is 0, it will be rounded to 1.
    pub fn with_size_and_hashers(bytes: usize, hashers: usize) -> Filter {
        // Convert `bytes` to number of `u64`s, and ceil to avoid case where the output is 0.
        let len = (bytes + 7) / 8;
        // Initialize a vector with zeros.
        let mut vec = Vec::with_capacity(len);
        for _ in 0..len {
            vec.push(AtomicU64::new(0));
        }

        Filter {
            bits: vec,
            // Set hashers to 1, if it is 0, as there must be at least one hash function.
            hashers: cmp::max(hashers, 1),
        }
    }

    /// Clear the Bloom filter.
    ///
    /// This removes every element from the Bloom filter.
    ///
    /// Note that it will not do so atomically, and it can remove elements inserted simulatenously
    /// to this function being called.
    pub fn clear(&self) {
        for i in &self.bits {
            // Clear the bits of this chunk.
            i.store(0, ORDERING);
        }
    }

    /// Insert an element into the Bloom filter.
    pub fn insert(&self, x: u64) {
        // Start at `x`.
        let mut h = x;
        // Run over the hashers.
        for _ in 0..self.hashers {
            // We use the hash function to generate a pseudorandom sequence, defining the different
            // hashes.
            h = hash(h);
            // Create a mask and OR the chunk chosen by `hash`.
            self.get(h).fetch_or(1 << (h % 8), ORDERING);
        }
    }

    /// Check if the Bloom filter potentially contains an element.
    ///
    /// This returns `true` if we're not sure if the filter contains `x` or not, and `false` if we
    /// know that the filter does not contain `x`.
    pub fn maybe_contains(&self, x: u64) -> bool {
        // Start at `x`.
        let mut h = x;

        // Go over the hashers.
        for _ in 0..self.hashers {
            // Again, the hashes are defined by a cuckoo sequence of repeatedly hashing.
            h = hash(h);
            // Short-circuit if the bit is not set.
            if self.get(h).load(ORDERING) & 1 << (h % 8) == 0 {
                // Since the bit of this hash value was not set, it is impossible that the filter
                // contains `x`, so we return `false`.
                return false;
            }
        }

        // Every bit was set, so the element might be in the filter.
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::thread;

    #[test]
    fn insert() {
        let filter = Filter::new(400, 4);
        filter.insert(3);
        filter.insert(5);
        filter.insert(7);
        filter.insert(13);

        assert!(!filter.maybe_contains(0));
        assert!(!filter.maybe_contains(1));
        assert!(!filter.maybe_contains(2));
        assert!(filter.maybe_contains(3));
        assert!(filter.maybe_contains(5));
        assert!(filter.maybe_contains(7));
        assert!(filter.maybe_contains(13));

        for i in 14..60 {
            assert!(!filter.maybe_contains(!i));
        }
    }

    #[test]
    fn clear() {
        let filter = Filter::new(400, 4);
        filter.insert(3);
        filter.insert(5);
        filter.insert(7);
        filter.insert(13);

        filter.clear();

        assert!(!filter.maybe_contains(0));
        assert!(!filter.maybe_contains(1));
        assert!(!filter.maybe_contains(2));
        assert!(!filter.maybe_contains(3));
        assert!(!filter.maybe_contains(5));
        assert!(!filter.maybe_contains(7));
        assert!(!filter.maybe_contains(13));
    }

    #[test]
    fn spam() {
        let filter = Arc::new(Filter::new(2000, 100));
        let mut joins = Vec::new();

        for _ in 0..16 {
            let filter = filter.clone();
            joins.push(thread::spawn(move || for i in 0..100 {
                filter.insert(i)
            }));
        }

        for i in joins {
            i.join().unwrap();
        }

        for i in 0..100 {
            assert!(filter.maybe_contains(i));
        }
        for i in 100..200 {
            assert!(!filter.maybe_contains(i));
        }
    }
}
