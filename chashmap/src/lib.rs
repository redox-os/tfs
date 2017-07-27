//! Concurrent hash maps.
//!
//! This crate implements concurrent hash maps, based on bucket-level multi-reader locks. It has
//! excellent performance characteristics¹ and supports resizing, in-place mutation and more.
//!
//! The API derives directly from `std::collections::HashMap`, giving it a familiar feel.
//!
//! ¹Note that it heavily depends on the behavior of your program, but in most cases, it's really
//!  good. In some (rare) cases you might want atomic hash maps instead.
//!
//! # How it works
//!
//! `chashmap` is not lockless, but it distributes locks across the map such that lock contentions
//! (which is what could make accesses expensive) are very rare.
//!
//! Hash maps consists of so called "buckets", which each defines a potential entry in the table.
//! The bucket of some key-value pair is determined by the hash of the key. By holding a read-write
//! lock for each bucket, we ensure that you will generally be able to insert, read, modify, etc.
//! with only one or two locking subroutines.
//!
//! There is a special-case: reallocation. When the table is filled up such that very few buckets
//! are free (note that this is "very few" and not "no", since the load factor shouldn't get too
//! high as it hurts performance), a global lock is obtained while rehashing the table. This is
//! pretty inefficient, but it rarely happens, and due to the adaptive nature of the capacity, it
//! will only happen a few times when the map has just been initialized.
//!
//! ## Collision resolution
//!
//! When two hashes collide, they cannot share the same bucket, so there must be an algorithm which
//! can resolve collisions. In our case, we use linear probing, which means that we take the bucket
//! following it, and repeat until we find a free bucket.
//!
//! This method is far from ideal, but superior methods like Robin-Hood hashing works poorly (if at
//! all) in a concurrent structure.
//!
//! # The API
//!
//! The API should feel very familiar, if you are used to the libstd hash map implementation. They
//! share many of the methods, and I've carefully made sure that all the items, which have similarly
//! named items in libstd, matches in semantics and behavior.

extern crate parking_lot;
extern crate owning_ref;

#[cfg(test)]
mod tests;

use owning_ref::{OwningHandle, OwningRef};
use parking_lot::{RwLock, RwLockWriteGuard, RwLockReadGuard};
use std::collections::hash_map;
use std::hash::{Hash, Hasher, BuildHasher};
use std::sync::atomic::{self, AtomicUsize};
use std::{mem, ops, cmp, fmt, iter};

/// The atomic ordering used throughout the code.
const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;
/// The length-to-capacity factor.
const LENGTH_MULTIPLIER: usize = 4;
/// The maximal load factor's numerator.
const MAX_LOAD_FACTOR_NUM: usize = 100 - 15;
/// The maximal load factor's denominator.
const MAX_LOAD_FACTOR_DENOM: usize = 100;
/// The default initial capacity.
const DEFAULT_INITIAL_CAPACITY: usize = 64;
/// The lowest capacity a table can have.
const MINIMUM_CAPACITY: usize = 8;

/// A bucket state.
///
/// Buckets are the bricks of hash tables. They represent a single entry into the table.
#[derive(Clone)]
enum Bucket<K, V> {
    /// The bucket contains a key-value pair.
    Contains(K, V),
    /// The bucket is empty and has never been used.
    ///
    /// Since hash collisions are resolved by jumping to the next bucket, some buckets can cluster
    /// together, meaning that they are potential candidates for lookups. Empty buckets can be seen
    /// as the delimiter of such cluters.
    Empty,
    /// The bucket was removed.
    ///
    /// The technique of distincting between "empty" and "removed" was first described by Knuth.
    /// The idea is that when you search for a key, you will probe over these buckets, since the
    /// key could have been pushed behind the removed element:
    ///```notest
    ///     Contains(k1, v1) // hash = h
    ///     Removed
    ///     Contains(k2, v2) // hash = h
    ///```
    /// If we stopped at `Removed`, we won't be able to find the second KV pair. So `Removed` is
    /// semantically different from `Empty`, as the search won't stop.
    ///
    /// However, we are still able to insert new pairs at the removed buckets.
    Removed,
}

impl<K, V> Bucket<K, V> {
    /// Is this bucket 'empty'?
    fn is_empty(&self) -> bool {
        if let Bucket::Empty = *self { true } else { false }
    }

    /// Is this bucket 'removed'?
    fn is_removed(&self) -> bool {
        if let Bucket::Removed = *self { true } else { false }
    }

    /// Is this bucket free?
    ///
    /// "Free" means that it can safely be replace by another bucket — namely that the bucket is
    /// not occupied.
    fn is_free(&self) -> bool {
        match *self {
            // The two replacable bucket types are removed buckets and empty buckets.
            Bucket::Removed | Bucket::Empty => true,
            // KV pairs can't be replaced as they contain data.
            Bucket::Contains(..) => false,
        }
    }

    /// Get the value (if any) of this bucket.
    ///
    /// This gets the value of the KV pair, if any. If the bucket is not a KV pair, `None` is
    /// returned.
    fn value(self) -> Option<V> {
        if let Bucket::Contains(_, val) = self {
            Some(val)
        } else { None }
    }

    /// Get a reference to the value of the bucket (if any).
    ///
    /// This returns a reference to the value of the bucket, if it is a KV pair. If not, it will
    /// return `None`.
    ///
    /// Rather than `Option`, it returns a `Result`, in order to make it easier to work with the
    /// `owning_ref` crate (`try_new` and `try_map` of `OwningHandle` and `OwningRef`
    /// respectively).
    fn value_ref(&self) -> Result<&V, ()> {
        if let Bucket::Contains(_, ref val) = *self {
            Ok(val)
        } else {
            Err(())
        }
    }

    /// Does the bucket match a given key?
    ///
    /// This returns `true` if the bucket is a KV pair with key `key`. If not, `false` is returned.
    fn key_matches(&self, key: &K) -> bool
    where K: PartialEq {
        if let Bucket::Contains(ref candidate_key, _) = *self {
            // Check if the keys matches.
            candidate_key == key
        } else {
            // The bucket isn't a KV pair, so we'll return false, since there is no key to test
            // against.
            false
        }
    }
}

/// The low-level representation of the hash table.
///
/// This is different from `CHashMap` in two ways:
///
/// 1. It is not wrapped in a lock, meaning that resizing and reallocation is not possible.
/// 2. It does not track the number of occupied buckets, making it expensive to obtain the load
///    factor.
struct Table<K, V> {
    /// The hash function builder.
    ///
    /// This randomly picks a hash function from some family of functions in libstd. This
    /// effectively eliminates the issue of hash flooding.
    hash_builder: hash_map::RandomState,
    /// The bucket array.
    ///
    /// This vector stores the buckets. The order in which they're stored is far from arbitrary: A
    /// KV pair `(key, val)`'s first priority location is at `self.hash(&key) % len`. If not
    /// possible, the next bucket is used, and this process repeats until the bucket is free (or
    /// the end is reached, in which we simply wrap around).
    buckets: Vec<RwLock<Bucket<K, V>>>,
}

impl<K, V> Table<K, V> {
    /// Create a table with a certain number of buckets.
    fn new(buckets: usize) -> Table<K, V> {
        // TODO: For some obscure reason `RwLock` doesn't implement `Clone`.

        // Fill a vector with `buckets` of `Empty` buckets.
        let mut vec = Vec::with_capacity(buckets);
        for _ in 0..buckets {
            vec.push(RwLock::new(Bucket::Empty));
        }

        Table {
            // Generate a hash function.
            hash_builder: hash_map::RandomState::new(),
            buckets: vec,
        }
    }

    /// Create a table with at least some capacity.
    fn with_capacity(cap: usize) -> Table<K, V> {
        Table::new(cmp::max(MINIMUM_CAPACITY, cap * LENGTH_MULTIPLIER))
    }
}

impl<K: PartialEq + Hash, V> Table<K, V> {
    /// Hash some key through the internal hash function.
    fn hash(&self, key: &K) -> usize {
        // Build the initial hash function state.
        let mut hasher = self.hash_builder.build_hasher();
        // Hash the key.
        key.hash(&mut hasher);
        // Cast to `usize`. Since the hash function returns `u64`, this cast won't ever cause
        // entropy less than the ouput space.
        hasher.finish() as usize
    }

    /// Scan from the first priority of a key until a match is found.
    ///
    /// This scans from the first priority of `key` (as defined by its hash), until a match is
    /// found (will wrap on end), i.e. `matches` returns `true` with the bucket as argument.
    ///
    /// The read guard from the RW-lock of the bucket is returned.
    fn scan<F>(&self, key: &K, matches: F) -> RwLockReadGuard<Bucket<K, V>>
    where F: Fn(&Bucket<K, V>) -> bool {
        // Hash the key.
        let hash = self.hash(key);

        // Start at the first priority bucket, and then move upwards, searching for the matching
        // bucket.
        for i in 0..self.buckets.len() {
            // Get the lock of the `i`'th bucket after the first priority bucket (wrap on end).
            let lock = self.buckets[(hash + i) % self.buckets.len()].read();

            // Check if it is a match.
            if matches(&lock) {
                // Yup. Return.
                return lock;
            }
        }
        panic!("`CHashMap` scan failed! No entry found.");
    }

    /// Scan from the first priority of a key until a match is found (mutable guard).
    ///
    /// This is similar to `scan`, but instead of an immutable lock guard, a mutable lock guard is
    /// returned.
    fn scan_mut<F>(&self, key: &K, matches: F) -> RwLockWriteGuard<Bucket<K, V>>
    where F: Fn(&Bucket<K, V>) -> bool {
        // Hash the key.
        let hash = self.hash(key);

        // Start at the first priority bucket, and then move upwards, searching for the matching
        // bucket.
        for i in 0..self.buckets.len() {
            // Get the lock of the `i`'th bucket after the first priority bucket (wrap on end).
            let lock = self.buckets[(hash + i) % self.buckets.len()].write();

            // Check if it is a match.
            if matches(&lock) {
                // Yup. Return.
                return lock;
            }
        }
        panic!("`CHashMap` scan_mut failed! No entry found.");
    }

    /// Scan from the first priority of a key until a match is found (bypass locks).
    ///
    /// This is similar to `scan_mut`, but it safely bypasses the locks by making use of the
    /// aliasing invariants of `&mut`.
    fn scan_mut_no_lock<F>(&mut self, key: &K, matches: F) -> &mut Bucket<K, V>
    where F: Fn(&Bucket<K, V>) -> bool {
        // Hash the key.
        let hash = self.hash(key);
        // TODO: To tame the borrowchecker, we fetch this in advance.
        let len = self.buckets.len();

        // Start at the first priority bucket, and then move upwards, searching for the matching
        // bucket.
        for i in 0..self.buckets.len() {
            // TODO: hacky hacky
            let idx = (hash + i) % len;

            // Get the lock of the `i`'th bucket after the first priority bucket (wrap on end).

            // Check if it is a match.
            if {
                let bucket = self.buckets[idx].get_mut();
                matches(&bucket)
            } {
                // Yup. Return.
                return self.buckets[idx].get_mut();
            }
        }
        panic!("`CHashMap` scan_mut_no_lock failed! No entry found.");
    }

    /// Find a bucket with some key, or a free bucket in same cluster.
    ///
    /// This scans for buckets with key `key`. If one is found, it will be returned. If none are
    /// found, it will return a free bucket in the same cluster.
    fn lookup_or_free(&self, key: &K) -> RwLockWriteGuard<Bucket<K, V>> {
        // Hash the key.
        let hash = self.hash(key);
        // The encountered free bucket.
        let mut free = None;

        // Start at the first priority bucket, and then move upwards, searching for the matching
        // bucket.
        for i in 0..self.buckets.len() {
            // Get the lock of the `i`'th bucket after the first priority bucket (wrap on end).
            let lock = self.buckets[(hash + i) % self.buckets.len()].write();

            if lock.key_matches(key) {
                // We found a match.
                return lock;
            } else if lock.is_empty() {
                // The cluster is over. Use the encountered free bucket, if any.
                return free.unwrap_or(lock);
            } else if lock.is_removed() && free.is_none() {
                // We found a free bucket, so we can store it to later (if we don't already have
                // one).
                free = Some(lock)
            }
        }

        free.expect("No free buckets found")
    }

    /// Lookup some key.
    ///
    /// This searches some key `key`, and returns a immutable lock guard to its bucket. If the key
    /// couldn't be found, the returned value will be an `Empty` cluster.
    fn lookup(&self, key: &K) -> RwLockReadGuard<Bucket<K, V>> {
        self.scan(key, |x| match *x {
            // We'll check that the keys does indeed match, as the chance of hash collisions
            // happening is inevitable
            Bucket::Contains(ref candidate_key, _) if key == candidate_key => true,
            // We reached an empty bucket, meaning that there are no more buckets, not even removed
            // ones, to search.
            Bucket::Empty => true,
            _ => false,
        })
    }

    /// Lookup some key, mutably.
    ///
    /// This is similar to `lookup`, but it returns a mutable guard.
    ///
    /// Replacing at this bucket is safe as the bucket will be in the same cluster of buckets as
    /// the first priority cluster.
    fn lookup_mut(&self, key: &K) -> RwLockWriteGuard<Bucket<K, V>> {
        self.scan_mut(key, |x| match *x {
            // We'll check that the keys does indeed match, as the chance of hash collisions
            // happening is inevitable
            Bucket::Contains(ref candidate_key, _) if key == candidate_key => true,
            // We reached an empty bucket, meaning that there are no more buckets, not even removed
            // ones, to search.
            Bucket::Empty => true,
            _ => false,
        })
    }

    /// Find a free bucket in the same cluster as some key.
    ///
    /// This means that the returned lock guard defines a valid, free bucket, where `key` can be
    /// inserted.
    fn find_free(&self, key: &K) -> RwLockWriteGuard<Bucket<K, V>> {
        self.scan_mut(key, |x| x.is_free())
    }

    /// Find a free bucket in the same cluster as some key (bypassing locks).
    ///
    /// This is similar to `find_free`, except that it safely bypasses locks through the aliasing
    /// guarantees of `&mut`.
    fn find_free_no_lock(&mut self, key: &K) -> &mut Bucket<K, V> {
        self.scan_mut_no_lock(key, |x| x.is_free())
    }

    /// Fill the table with data from another table.
    ///
    /// This is used to efficiently copy the data of `table` into `self`.
    ///
    /// # Important
    ///
    /// The table should be empty for this to work correctly/logically.
    fn fill(&mut self, table: Table<K, V>) {
        // Run over all the buckets.
        for i in table.buckets {
            // We'll only transfer the bucket if it is a KV pair.
            if let Bucket::Contains(key, val) = i.into_inner() {
                // Find a bucket where the KV pair can be inserted.
                let mut bucket = self.scan_mut_no_lock(&key, |x| match *x {
                    // Halt on an empty bucket.
                    Bucket::Empty => true,
                    // We'll assume that the rest of the buckets either contains other KV pairs (in
                    // particular, no buckets have been removed in the newly construct table).
                    _ => false,
                });

                // Set the bucket to the KV pair.
                *bucket = Bucket::Contains(key, val);
            }
        }
    }
}

impl<K: Clone, V: Clone> Clone for Table<K, V> {
    fn clone(&self) -> Table<K, V> {
        Table {
            // Since we copy plainly without rehashing etc., it is important that we keep the same
            // hash function.
            hash_builder: self.hash_builder.clone(),
            // Lock and clone every bucket individually.
            buckets: self.buckets.iter().map(|x| RwLock::new(x.read().clone())).collect(),
        }
    }
}

impl<K: fmt::Debug, V: fmt::Debug> fmt::Debug for Table<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // create a debug map and fill with entries
        let mut map = f.debug_map();
        // We'll just run over all buckets and output one after one.
        for i in &self.buckets {
            // Acquire the lock.
            let lock = i.read();
            // Check if the bucket actually contains anything.
            if let Bucket::Contains(ref key, ref val) = *lock {
                // add this entry to the map
                map.entry(key, val);
            }
        }
        map.finish()
    }
}

/// An iterator over the entries of some table.
pub struct IntoIter<K, V> {
    /// The inner table.
    table: Table<K, V>,
}

impl<K, V> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<(K, V)> {
        // We own the table, and can thus do what we want with it. We'll simply pop from the
        // buckets until we find a bucket containing data.
        while let Some(bucket) = self.table.buckets.pop() {
            // We can bypass dem ebil locks.
            if let Bucket::Contains(key, val) = bucket.into_inner() {
                // The bucket contained data, so we'll return the pair.
                return Some((key, val));
            }
        }

        // We've exhausted all the buckets, and no more data could be found.
        None
    }
}

impl<K, V> IntoIterator for Table<K, V> {
    type Item = (K, V);
    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> IntoIter<K, V> {
        IntoIter {
            table: self,
        }
    }
}

/// A RAII guard for reading an entry of a hash map.
///
/// This is an access type dereferencing to the inner value of the entry. It will handle unlocking
/// on drop.
pub struct ReadGuard<'a, K: 'a, V: 'a> {
    /// The inner hecking long type.
    inner: OwningRef<OwningHandle<RwLockReadGuard<'a, Table<K, V>>, RwLockReadGuard<'a, Bucket<K, V>>>, V>,
}

impl<'a, K, V> ops::Deref for ReadGuard<'a, K, V> {
    type Target = V;

    fn deref(&self) -> &V {
        &self.inner
    }
}

impl<'a, K, V: PartialEq> cmp::PartialEq for ReadGuard<'a, K, V> {
    fn eq(&self, other: &ReadGuard<'a, K, V>) -> bool {
        self == other
    }
}
impl<'a, K, V: Eq> cmp::Eq for ReadGuard<'a, K, V> {}

impl<'a, K: fmt::Debug, V: fmt::Debug> fmt::Debug for ReadGuard<'a, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ReadGuard({:?})", &**self)
    }
}

/// A mutable RAII guard for reading an entry of a hash map.
///
/// This is an access type dereferencing to the inner value of the entry. It will handle unlocking
/// on drop.
pub struct WriteGuard<'a, K: 'a, V: 'a> {
    /// The inner hecking long type.
    inner: OwningHandle<OwningHandle<RwLockReadGuard<'a, Table<K, V>>, RwLockWriteGuard<'a, Bucket<K, V>>>, &'a mut V>,
}

impl<'a, K, V> ops::Deref for WriteGuard<'a, K, V> {
    type Target = V;

    fn deref(&self) -> &V {
        &self.inner
    }
}

impl<'a, K, V> ops::DerefMut for WriteGuard<'a, K, V> {
    fn deref_mut(&mut self) -> &mut V {
        &mut self.inner
    }
}

impl<'a, K, V: PartialEq> cmp::PartialEq for WriteGuard<'a, K, V> {
    fn eq(&self, other: &WriteGuard<'a, K, V>) -> bool {
        self == other
    }
}
impl<'a, K, V: Eq> cmp::Eq for WriteGuard<'a, K, V> {}

impl<'a, K: fmt::Debug, V: fmt::Debug> fmt::Debug for WriteGuard<'a, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "WriteGuard({:?})", &**self)
    }
}

/// A concurrent hash map.
///
/// This type defines a concurrent associative array, based on hash tables with linear probing and
/// dynamic resizing.
///
/// The idea is to let each entry hold a multi-reader lock, effectively limiting lock contentions
/// to writing simultaneously on the same entry, and resizing the table.
///
/// It is not an atomic or lockless hash table, since such construction is only useful in very few
/// cases, due to limitations on in-place operations on values.
pub struct CHashMap<K, V> {
    /// The inner table.
    table: RwLock<Table<K, V>>,
    /// The total number of KV pairs in the table.
    ///
    /// This is used to calculate the load factor.
    len: AtomicUsize,
}

impl<K, V> CHashMap<K, V> {
    /// Create a new hash map with a certain capacity.
    ///
    /// "Capacity" means the amount of entries the hash map can hold before reallocating. This
    /// function allocates a hash map with at least the capacity of `cap`.
    pub fn with_capacity(cap: usize) -> CHashMap<K, V> {
        CHashMap {
            // Start at 0 KV pairs.
            len: AtomicUsize::new(0),
            // Make a new empty table. We will make sure that it is at least one.
            table: RwLock::new(Table::with_capacity(cap)),
        }
    }

    /// Create a new hash map.
    ///
    /// This creates a new hash map with some fixed initial capacity.
    pub fn new() -> CHashMap<K, V> {
        CHashMap::with_capacity(DEFAULT_INITIAL_CAPACITY)
    }

    /// Get the number of entries in the hash table.
    ///
    /// This is entirely atomic, and will not acquire any locks.
    ///
    /// This is guaranteed to reflect the number of entries _at this particular moment.
    pub fn len(&self) -> usize {
        self.len.load(ORDERING)
    }

    /// Get the capacity of the hash table.
    ///
    /// The capacity is equal to the number of entries the table can hold before reallocating.
    pub fn capacity(&self) -> usize {
        self.buckets() * MAX_LOAD_FACTOR_NUM / MAX_LOAD_FACTOR_DENOM
    }

    /// Get the number of buckets of the hash table.
    ///
    /// "Buckets" refers to the amount of potential entries in the inner table. It is different
    /// from capacity, in the sense that the map cannot hold this number of entries, since it needs
    /// to keep the load factor low.
    pub fn buckets(&self) -> usize {
        self.table.read().buckets.len()
    }

    /// Is the hash table empty?
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear the map.
    ///
    /// This clears the hash map and returns the previous version of the map.
    ///
    /// It is relatively efficient, although it needs to write lock a RW lock.
    pub fn clear(&self) -> CHashMap<K, V> {
        // Acquire a writable lock.
        let mut lock = self.table.write();
        CHashMap {
            // Replace the old table with an empty initial table.
            table: RwLock::new(mem::replace(&mut *lock, Table::new(DEFAULT_INITIAL_CAPACITY))),
            // Replace the length with 0 and use the old length.
            len: AtomicUsize::new(self.len.swap(0, ORDERING)),
        }
    }

    /// Deprecated. Do not use.
    #[deprecated]
    pub fn filter<F>(&self, predicate: F)
    where F: Fn(&K, &V) -> bool {
        // Following the naming conventions of the standard library...
        self.retain(predicate)
    }

    /// Filter the map based on some predicate.
    ///
    /// This tests every entry in the hash map by closure `predicate`. If it returns `true`, the
    /// map will retain the entry. If not, the entry will be removed.
    ///
    /// This won't lock the table. This can be a major performance trade-off, as it means that it
    /// must lock on every table entry. However, it won't block other operations of the table,
    /// while filtering.
    pub fn retain<F>(&self, predicate: F)
    where F: Fn(&K, &V) -> bool {
        // Acquire the read lock to the table.
        let table = self.table.read();
        // Run over every bucket and apply the filter.
        for bucket in &table.buckets {
            // Acquire the read lock, which we will upgrade if necessary.
            // TODO: Use read lock and upgrade later.
            let mut lock = bucket.write();
            // Skip the free buckets.
            // TODO: Fold the `if` into the `match` when the borrowck gets smarter.
            if match *lock {
                Bucket::Contains(ref key, ref val) => !predicate(key, val),
                _ => false,
            } {
                // Predicate didn't match. Set the bucket to removed.
                *lock = Bucket::Removed;
                // Decrement the length to account for the removed bucket.
                // TODO: Can we somehow bundle these up to reduce the overhead of atomic
                //       operations? Storing in a local variable and then subtracting causes
                //       issues with consistency.
                self.len.fetch_sub(1, ORDERING);
            }
        }
    }
}

impl<K: PartialEq + Hash, V> CHashMap<K, V> {
    /// Get the value of some key.
    ///
    /// This will lookup the entry of some key `key`, and acquire the read-only lock. This means
    /// that all other parties are blocked from _writing_ (not reading) this value while the guard
    /// is held.
    pub fn get(&self, key: &K) -> Option<ReadGuard<K, V>> {
        // Acquire the read lock and lookup in the table.
        if let Ok(inner) = OwningRef::new(
            OwningHandle::new_with_fn(self.table.read(), |x| unsafe { &*x }.lookup(key))
        ).try_map(|x| x.value_ref()) {
            // The bucket contains data.
            Some(ReadGuard {
                inner: inner,
            })
        } else {
            // The bucket is empty/removed.
            None
        }
    }

    /// Get the (mutable) value of some key.
    ///
    /// This will lookup the entry of some key `key`, and acquire the writable lock. This means
    /// that all other parties are blocked from both reading and writing this value while the guard
    /// is held.
    pub fn get_mut(&self, key: &K) -> Option<WriteGuard<K, V>> {
        // Acquire the write lock and lookup in the table.
        if let Ok(inner) = OwningHandle::try_new(OwningHandle::new_with_fn(
            self.table.read(),
            |x| unsafe { &*x }.lookup_mut(key)),
            |x| {
                if let &mut Bucket::Contains(_, ref mut val) = unsafe {
                    &mut *(x as *mut Bucket<K, V>)
                } {
                    // The bucket contains data.
                    Ok(val)
                } else {
                    // The bucket is empty/removed.
                    Err(())
                }
            }
        ) {
            Some(WriteGuard {
                inner: inner,
            })
        } else { None }
    }

    /// Does the hash map contain this key?
    pub fn contains_key(&self, key: &K) -> bool {
        // Acquire the lock.
        let lock = self.table.read();
        // Look the key up in the table
        let bucket = lock.lookup(key);
        // Test if it is free or not.
        !bucket.is_free()

        // fuck im sleepy rn
    }

    /// Insert a **new** entry.
    ///
    /// This inserts an entry, which the map does not already contain, into the table. If the entry
    /// exists, the old entry won't be replaced, nor will an error be returned. It will possibly
    /// introduce silent bugs.
    ///
    /// To be more specific, it assumes that the entry does not already exist, and will simply skip
    /// to the end of the cluster, even if it does exist.
    ///
    /// This is faster than e.g. `insert`, but should only be used, if you know that the entry
    /// doesn't already exist.
    ///
    /// # Warning
    ///
    /// Only use this, if you know what you're doing. This can easily introduce very complex logic
    /// errors.
    ///
    /// For most other purposes, use `insert`.
    ///
    /// # Panics
    ///
    /// This might perform checks in debug mode testing if the key exists already.
    pub fn insert_new(&self, key: K, val: V) {
        debug_assert!(!self.contains_key(&key), "Hash table contains already key, contrary to \
                      the assumptions about `insert_new`'s arguments.");

        // Expand and lock the table. We need to expand to ensure the bounds on the load factor.
        let lock = self.table.read();
        {
            // Find the free bucket.
            let mut bucket = lock.find_free(&key);

            // Set the bucket to the new KV pair.
            *bucket = Bucket::Contains(key, val);
        }
        // Expand the table (we know beforehand that the entry didn't already exist).
        self.expand(lock);
    }

    /// Replace an existing entry, or insert a new one.
    ///
    /// This will replace an existing entry and return the old entry, if any. If no entry exists,
    /// it will simply insert the new entry and return `None`.
    pub fn insert(&self, key: K, val: V) -> Option<V> {
        let ret;
        // Expand and lock the table. We need to expand to ensure the bounds on the load factor.
        let lock = self.table.read();
        {
            // Lookup the key or a free bucket in the inner table.
            let mut bucket = lock.lookup_or_free(&key);

            // Replace the bucket.
            ret = mem::replace(&mut *bucket, Bucket::Contains(key, val)).value();
        }

        // Expand the table if no bucket was overwritten (i.e. the entry is fresh).
        if ret.is_none() {
            self.expand(lock);
        }

        ret
    }

    /// Insert or update.
    ///
    /// This looks up `key`. If it exists, the reference to its value is passed through closure
    /// `update`.  If it doesn't exist, the result of closure `insert` is inserted.
    pub fn upsert<F, G>(&self, key: K, insert: F, update: G)
    where
        F: FnOnce() -> V,
        G: FnOnce(&mut V),
    {
        // Expand and lock the table. We need to expand to ensure the bounds on the load factor.
        let lock = self.table.read();
        {
            // Lookup the key or a free bucket in the inner table.
            let mut bucket = lock.lookup_or_free(&key);

            match *bucket {
                // The bucket had KV pair!
                Bucket::Contains(_, ref mut val) => {
                    // Run it through the closure.
                    update(val);
                    // TODO: We return to stop the borrowck to yell at us. This prevents the control flow
                    //       from reaching the expansion after the match if it has been right here.
                    return;
                },
                // The bucket was empty, simply insert.
                ref mut x => *x = Bucket::Contains(key, insert()),
            }
        }

        // Expand the table (this will only happen if the function haven't returned yet).
        self.expand(lock);
    }

    /// Map or insert an entry.
    ///
    /// This sets the value associated with key `key` to `f(Some(old_val))` (if it returns `None`,
    /// the entry is removed) if it exists. If it does not exist, it inserts it with value
    /// `f(None)`, unless the closure returns `None`.
    ///
    /// Note that if `f` returns `None`, the entry of key `key` is removed unconditionally.
    pub fn alter<F>(&self, key: K, f: F)
    where F: FnOnce(Option<V>) -> Option<V> {
        // Expand and lock the table. We need to expand to ensure the bounds on the load factor.
        let lock = self.table.read();
        {
            // Lookup the key or a free bucket in the inner table.
            let mut bucket = lock.lookup_or_free(&key);

            match mem::replace(&mut *bucket, Bucket::Removed) {
                Bucket::Contains(_, val) => if let Some(new_val) = f(Some(val)) {
                    // Set the bucket to a KV pair with the new value.
                    *bucket = Bucket::Contains(key, new_val);
                    // No extension required, as the bucket already had a KV pair previously.
                    return;
                } else {
                    // The old entry was removed, so we decrement the length of the map.
                    self.len.fetch_sub(1, ORDERING);
                    // TODO: We return as a hack to avoid the borrowchecker from thinking we moved a
                    //       referenced object. Namely, under this match arm the expansion after the match
                    //       statement won't ever be reached.
                    return;
                },
                _ => if let Some(new_val) = f(None) {
                    // The previously free cluster will get a KV pair with the new value.
                    *bucket = Bucket::Contains(key, new_val);
                } else { return; },
            }
        }

        // A new entry was inserted, so naturally, we expand the table.
        self.expand(lock);
    }

    /// Remove an entry.
    ///
    /// This removes and returns the entry with key `key`. If no entry with said key exists, it
    /// will simply return `None`.
    pub fn remove(&self, key: &K) -> Option<V> {
        // Acquire the read lock of the table.
        let lock = self.table.read();

        // Lookup the table, mutably.
        let mut bucket = lock.lookup_mut(&key);
        // Remove the bucket.
        match &mut *bucket {
            // There was nothing to remove.
            &mut Bucket::Removed | &mut Bucket::Empty => None,
            // TODO: We know that this is a `Bucket::Contains` variant, but to bypass borrowck
            //       madness, we do weird weird stuff.
            bucket => {
                // Decrement the length of the map.
                self.len.fetch_sub(1, ORDERING);

                // Set the bucket to "removed" and return its value.
                mem::replace(bucket, Bucket::Removed).value()
            },
        }
    }

    /// Reserve additional space.
    ///
    /// This reserves additional `additional` buckets to the table. Note that it might reserve more
    /// in order make reallocation less common.
    pub fn reserve(&self, additional: usize) {
        // Get the new length.
        let len = self.len() + additional;
        // Acquire the write lock (needed because we'll mess with the table).
        let mut lock = self.table.write();
        // Handle the case where another thread has resized the table while we were acquiring the
        // lock.
        if lock.buckets.len() < len * LENGTH_MULTIPLIER {
            // Swap the table out with a new table of desired size (multiplied by some factor).
            let table = mem::replace(&mut *lock, Table::with_capacity(len));
            // Fill the new table with the data from the old table.
            lock.fill(table);
        }
    }

    /// Shrink the capacity of the map to reduce space usage.
    ///
    /// This will shrink the capacity of the map to the needed amount (plus some additional space
    /// to avoid reallocations), effectively reducing memory usage in cases where there is
    /// excessive space.
    ///
    /// It is healthy to run this once in a while, if the size of your hash map changes a lot (e.g.
    /// has a high maximum case).
    pub fn shrink_to_fit(&self) {
        // Acquire the write lock (needed because we'll mess with the table).
        let mut lock = self.table.write();
        // Swap the table out with a new table of desired size (multiplied by some factor).
        let table = mem::replace(&mut *lock, Table::with_capacity(self.len()));
        // Fill the new table with the data from the old table.
        lock.fill(table);
    }

    /// Increment the size of the hash map and expand it so one more entry can fit in.
    ///
    /// This returns the read lock, such that the caller won't have to acquire it twice.
    fn expand(&self, lock: RwLockReadGuard<Table<K, V>>) {
        // Increment the length to take the new element into account.
        let len = self.len.fetch_add(1, ORDERING) + 1;

        // Extend if necessary. We multiply by some constant to adjust our load factor.
        if len * MAX_LOAD_FACTOR_DENOM > lock.buckets.len() * MAX_LOAD_FACTOR_NUM {
            // Drop the read lock to avoid deadlocks when acquiring the write lock.
            drop(lock);
            // Reserve 1 entry in space (the function will handle the excessive space logic).
            self.reserve(1);
        }
    }
}

impl<K, V> Default for CHashMap<K, V> {
    fn default() -> CHashMap<K, V> {
        // Forward the call to `new`.
        CHashMap::new()
    }
}

impl<K: Clone, V: Clone> Clone for CHashMap<K, V> {
    fn clone(&self) -> CHashMap<K, V> {
        CHashMap {
            table: RwLock::new(self.table.read().clone()),
            len: AtomicUsize::new(self.len.load(ORDERING)),
        }
    }
}

impl<K: fmt::Debug, V: fmt::Debug> fmt::Debug for CHashMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (*self.table.read()).fmt(f)
    }
}

impl<K, V> IntoIterator for CHashMap<K, V> {
    type Item = (K, V);
    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> IntoIter<K, V> {
        self.table.into_inner().into_iter()
    }
}

impl<K: PartialEq + Hash, V> iter::FromIterator<(K, V)> for CHashMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> CHashMap<K, V> {
        // TODO: This step is required to obtain the length of the iterator. Eliminate it.
        let vec: Vec<_> = iter.into_iter().collect();
        let len = vec.len();

        // Start with an empty table.
        let mut table = Table::with_capacity(len);
        // Fill the table with the pairs from the iterator.
        for (key, val) in vec {
            // Insert the KV pair. This is fine, as we are ensured that there are no duplicates in
            // the iterator.
            let bucket = table.find_free_no_lock(&key);
            *bucket = Bucket::Contains(key, val);
        }

        CHashMap {
            table: RwLock::new(table),
            len: AtomicUsize::new(len),
        }
    }
}
