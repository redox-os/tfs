//! An efficient implementation of concurrent (lockless) the PLRU cache replacement policy.
//!
//! # Cache replacement
//!
//! Cache replacement policies are used to determine which cache lines (buffers), that should be
//! replaced because they're no longer "hot" (likely to be used in the near future). A number of
//! different approaches exist.
//!
//! LRU is the name of the cache replacement policies which will replace the cache line which was
//! used the longest time ago.
//!
//! PLRU is the name of the class of cache replacement deriving from LRU, but instead of using an
//! exact measure they use an approximate measure of the recently used lines. This allows for much
//! more efficient cache management (LRU cache management is notoriously slow) on the cost of
//! possibly worse cache policies. PLRU caches are used in many major CPUs to maintain CPU cache
//! lines.
//!
//! We use a specific version of PLRU: BPLRU or bit PLRU. Tree PLRU is an alternative, but it is
//! not easy to make concurrent.
//!
//! # Usage
//!
//! ## Runtime defined number of cache lines
//!
//! This can be achieved through `plru::create()` unless the `no_std` feature is enabled.
//!
//! ## How fixed-size arrays are handled
//!
//! Our implementation is generic over fixed-size arrays (or heap allocated vectors) without
//! relying on any external library. We abstract over `AsRef<[AtomicU64]>` in order to allow a wide
//! varity of different forms of arrays.
//!
//! For convenience, we define a bunch of type aliases for various sizes of caches.
//!
//! # Implementation details
//!
//! This implementation of BPLRU makes use of atomic integers to store the bit flags, and thus make
//! up a concurrent and entirely lockless cache manager with excellent performance.
//!
//! The cache lines are organized into 64-bit integers storing the bit flags ("hot" and "cold").
//! Each of these flags are called "bulks". Whenever a cache line is touched (used), its bit flag
//! is said.
//!
//! Finding the cache line to replace is simply done by finding an unset bit in one of the bulks.
//! If all cache lines in the bulk are hot, they're flipped so that they're all cold. This process
//! is called cache inflation.
//!
//! In order to avoid double cache replacement, we use a counter which is used to maximize the time
//! between the same bulk being used to find a cache line replacement.
//!
//! The algorithms are described in detail in the documentation of each method.
//!
//! # In short...
//!
//! In short, the algorithm works by cache lines holding an entry until their slot (bulk) is
//! filled, which will (upon cache replacement) lead to every cache line in that bulk being assumed
//! to be inactive until they reclaim their place.
//!
//! An analogy is parking lots. We only care about parking lot space if it is filled (if it isn't,
//! we can just place our car, and we're good). When it's filled, we need to make sure that the
//! owner of every car has been there in the last N hours (or: since the parking lot got filled) to
//! avoid owners occupying places forever. When new cars come in and a car have been there for
//! enough time, it have to be forcibly removed to open up new space.
//!
//! Now, think of each parking lot as a bulk. And each car as a cache line.
//!
//! # Code quality
//!
//! ✔ Well-tested
//!
//! ✔ Well-documented
//!
//! ✔ Examples
//!
//! ✔ Conforms to Rust's style guidelines
//!
//! ✔ Idiomatic Rust
//!
//! ✔ Production-quality

#![cfg_attr(features = "no_std", no_std)]
#![feature(integer_atomics)]
#![warn(missing_docs)]

extern crate core;

use core::fmt;
use core::sync::atomic::{self, AtomicU64, AtomicU8};

/// The atomic ordering we use throughout the code.
const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;

/// A micro cache (64 lines).
pub type MicroCache = Cache<[AtomicU64; 1]>;
/// A small cache (128 lines).
pub type SmallCache = Cache<[AtomicU64; 2]>;
/// A medium size cache (256 lines).
pub type MediumCache = Cache<[AtomicU64; 4]>;
/// A big cache (512 lines).
pub type BigCache = Cache<[AtomicU64; 8]>;
/// A huge cache (2048 lines).
pub type HugeCache = Cache<[AtomicU64; 32]>;
/// A dynamic cache.
pub type DynamicCache = Cache<Box<[AtomicU64]>>;

/// Create a heap allocated cache of a fixed (but dynamic) number of cache lines.
///
/// Ideally, 64 should divide `len` to optimize space efficiency.
#[cfg(not(features = "no_std"))]
pub fn create(lines: usize) -> DynamicCache {
    // Unfortunately, `AtomicU64` doesn't implement `Clone`, so we cannot use the `vec![]` macro.
    // We need to manually construct our vector.

    // We divide it by 64 to get the target length of the vector, but instead of rounding down, we
    // ensure that we have the right length by using rounding up division through this simple trick.
    let len = (lines + 63) / 64;

    // Allocate a `len` capacity vector.
    let mut vec = Vec::with_capacity(len);

    // Repeat `len` times and push the value.
    for _ in 0..len {
        vec.push(AtomicU64::default());
    }

    Cache::new(vec.into_boxed_slice())
}

/// A pseudo-LRU cache tracker.
///
/// This manages a set of cache lines (enumerated by `usize`) in an efficient manner, such that you
/// can touch cache lines (mark usage) and efficiently find a cache line which can be replaced
/// (unlikely to be used in the near future).
#[derive(Default)]
pub struct Cache<B> {
    /// The LRU bit flags representing the "recently used" lines.
    ///
    /// The bits are broken down into 64-bit "bulks", which allows us to handle them efficiently
    /// atomically.
    bulks: B,
    /// A counter which is incremented on every new `replace`.
    ///
    /// This is used to reduce the chance of choosing the same replacement cache line twice.
    counter: AtomicU8,
}

impl<B: AsRef<[AtomicU64]>> Cache<B> {
    /// Create a new cache based on some array.
    ///
    /// Generally, this should not be used unless you're building abstractions. You should likely
    /// use `Cache::<SomeType>::default()` or `plru::create()` instead.
    ///
    /// # Example
    ///
    /// ```rust
    /// use plru::Cache;
    ///
    /// Cache::new([Default::default(), Default::default()]);
    /// ```
    pub fn new(bulks: B) -> Cache<B> {
        Cache {
            bulks: bulks,
            counter: AtomicU8::default(),
        }
    }

    /// Touch the n'th cache line.
    ///
    /// Whenever you modify/use the cache line, you should call `touch` in order to mark that it
    /// was recently used.
    ///
    /// Each cache line has a bitflag defining if it was recently used. This will set said flag.
    ///
    /// # Example
    ///
    /// ```rust
    /// let cache = plru::SmallCache::default();
    ///
    /// cache.touch(10);
    /// assert!(cache.is_hot(10));
    /// ```
    pub fn touch(&self, n: usize) {
        // We OR our mask together with the bulk in order to set the bit in question.
        self.bulks.as_ref()[n / 64].fetch_or(1 << (n % 64), ORDERING);
    }

    /// Trash the n'th cache line.
    ///
    /// Trashing is generally only used if you _know_ that this line is not going to be used later
    /// on.
    ///
    /// Trashing will merely mark this line as cold, and hence be queued for replacement until it
    /// is used again. As such, it is not a terrible performance loss if you trash a line which is
    /// used later, and decision can be made heuristically ("this is likely not going to be used
    /// later again").
    ///
    /// # Example
    ///
    /// ```rust
    /// let cache = plru::SmallCache::default();
    ///
    /// cache.touch(10);
    /// assert!(cache.is_hot(10));
    ///
    /// cache.trash(10);
    /// assert!(!cache.is_hot(10));
    /// ```
    pub fn trash(&self, n: usize) {
        // We use a mask and atomic AND in order to set the bit to zero.
        self.bulks.as_ref()[n / 64].fetch_and(!(1 << (n % 64)), ORDERING);
    }

    /// Find the approximately least recently used cache line to replace.
    ///
    /// A particular bulk is selected based on a counter incremented on every `replace` call. The
    /// first unset bit of this bulk determines the cold cache line we will return. If all the
    /// flags in the 64-bit bulk are set, the whole bulk will be reset to zero in order to inflate
    /// the cache.
    ///
    /// This is approximately the least-recently-used cache line.
    ///
    /// Note that it will not set the found bit. If you use the line right after requesting it, you
    /// still need to call `touch`. In fact, it will always return a cold line.
    ///
    /// You cannot rely on uniqueness of the output. It might return the same result twice,
    /// although it is unlikely.
    ///
    /// # Example
    ///
    /// ```rust
    /// let cache = plru::MediumCache::default();
    /// cache.touch(10);
    /// cache.touch(20);
    /// cache.touch(1);
    ///
    /// assert_ne!(cache.replace(), 1);
    /// assert_ne!(cache.replace(), 20);
    /// assert_ne!(cache.replace(), 10);
    /// ```
    pub fn replace(&self) -> usize {
        // In order to avoid returning the same result, we use a counter which wraps. Incrementing
        // this counter and using it to index the bulks maximizes the time spend inbetween the
        // allocations of the replacement line.
        let counter = self.counter.fetch_add(1, ORDERING) as usize % self.bulks.as_ref().len();

        // Load the bulk. If it turns out that every bit is set, this bulk will be inflated by
        // setting zeroing the bulk, so that all cache lines in this bulk are marked cold.
        let bulk = self.bulks.as_ref()[counter].compare_and_swap(!0, 0, ORDERING);

        // Find the first set bit in the bulk. The modulo 64 here is a neat hack, which allows us
        // to eliminate a branch. In particular, the ffz will only return 64 if all bits are set,
        // which is only the case if the old bulk value was inflated. By doing modulo 64, we map 64
        // to 0, which is not set if the bulk was inflated (all lines in the bulk are marked
        // "cold").
        let ffz = (!bulk).trailing_zeros() % 64;

        // Calculate the place of the line.
        counter * 64 + ffz as usize
    }

    /// Find the number of cache lines in this cache.
    ///
    /// This is subject equal to the length of `B` mutliplied by 64.
    ///
    /// # Example
    ///
    /// ```rust
    /// assert_eq!(plru::create(10).len(), 64);
    /// assert_eq!(plru::create(64).len(), 64);
    /// assert_eq!(plru::create(65).len(), 128);
    /// ```
    pub fn len(&self) -> usize {
        self.bulks.as_ref().len() * 64
    }

    /// Is the n'th cache line hot?
    ///
    /// This returns a boolean defining if it has recently be used or not. `true` means that the
    /// cache line is registered as "hot" and `false` that it is registered as "cold".
    ///
    /// # Example
    ///
    /// ```rust
    /// let cache = plru::MicroCache::default();
    /// cache.touch(2);
    ///
    /// assert!(cache.is_hot(2));
    /// assert!(!cache.is_hot(3));
    /// ```
    pub fn is_hot(&self, n: usize) -> bool {
        // Load the bulk and mask it to find out if the bit is set.
        self.bulks.as_ref()[n / 64].load(ORDERING) & (1 << (n % 64)) != 0
    }
}

impl<B: AsRef<[AtomicU64]>> fmt::Debug for Cache<B> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for i in self.bulks.as_ref() {
            write!(f, "{:b},", i.load(ORDERING))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(not(features = "no_std"))]
    fn create() {
        assert_eq!(super::create(10).len(), 64);
        assert_eq!(super::create(64).len(), 64);
        assert_eq!(super::create(63).len(), 64);
        assert_eq!(super::create(1).len(), 64);
        assert_eq!(super::create(65).len(), 128);
        assert_eq!(super::create(128).len(), 128);
        assert_eq!(super::create(129).len(), 192);
        assert_eq!(super::create(192).len(), 192);
        assert_eq!(super::create(256).len(), 256);
    }

    #[test]
    fn default() {
        let cache = super::BigCache::default();
        for i in 0..512 {
            assert!(!cache.is_hot(i));
        }
    }

    #[test]
    fn inflation() {
        let cache = super::SmallCache::default();

        // Touch all the cache lines in a bulk.

        cache.touch(0);

        for i in 1..64 {
            assert!(cache.is_hot(i - 1));
            cache.touch(i);
        }

        // Find a replacement line.
        assert_eq!(cache.replace(), 0);

        // Since every line was hot before, inflation should have occured. Check that every line is
        // cold.
        for i in 0..64 {
            assert!(!cache.is_hot(i));
        }
    }

    #[test]
    fn simple_replace() {
        let cache = super::SmallCache::default();

        assert_eq!(cache.replace(), 0);
        assert_eq!(cache.replace(), 64);
        assert_eq!(cache.replace(), 0);
        assert_eq!(cache.replace(), 64);
        cache.touch(0);
        cache.touch(64);
        assert_eq!(cache.replace(), 1);
        assert_eq!(cache.replace(), 65);
        cache.touch(1);
        cache.touch(65);
        assert_eq!(cache.replace(), 2);
        assert_eq!(cache.replace(), 66);
    }

    #[test]
    fn replace_and_touch() {
        let cache = super::SmallCache::default();

        for _ in 0..128 {
            let r = cache.replace();

            assert!(!cache.is_hot(r));
            cache.touch(r);
            assert!(cache.is_hot(r));
        }
    }

    #[test]
    fn replace_touch_trash() {
        let cache = super::SmallCache::default();

        for _ in 0..1000 {
            let r = cache.replace();
            cache.touch(r);
            assert!(cache.is_hot(r));
            cache.trash(r);
            assert!(!cache.is_hot(r));
        }
    }

    #[test]
    fn replace_cold() {
        let cache = super::SmallCache::default();

        for i in 0..1000 {
            let r = cache.replace();

            assert!(!cache.is_hot(r));

            if i % 2 == 0 {
                cache.touch(r);
            }
        }
    }

    #[test]
    fn trash_cold() {
        let cache = super::SmallCache::default();

        for i in 0..128 {
            cache.trash(i);
            assert!(!cache.is_hot(i));
        }
    }

    #[test]
    fn cache_sizes() {
        let a = super::MicroCache::default();
        let b = super::SmallCache::default();
        let c = super::MediumCache::default();
        let d = super::BigCache::default();
        let e = super::HugeCache::default();

        assert!(a.len() < b.len());
        assert!(b.len() < c.len());
        assert!(c.len() < d.len());
        assert!(d.len() < e.len());
    }

    #[test]
    #[should_panic]
    fn line_oob() {
        let cache = super::SmallCache::default();
        cache.touch(128);
    }
}
