//! Implementation of a lock-free, atomic hash table.
//!
//! This crate provides a high-performance implementation of a completely
//! lock-free (no mutexes, no spin-locks, or the alike) hash table.
//!
//! The only instruction we use is CAS, which allows us to atomically update
//! the table.
//!
//! # Design
//!
//! It is structured as a 256-radix tree with a pseudorandom permutation
//! applied to the key.  Contrary to open addressing, this approach is entirely
//! lock-free and need not reallocation.
//!
//! The permutation is a simple table+XOR based length-padded function, which
//! is applied to avoid excessive depth (this is what makes it a "hash table").
//!
//! See [this blog post](https://ticki.github.io/blog/an-atomic-hash-table/)
//! for details.

extern crate conc;

mod sponge;
mod table;

use std::hash::Hash;
use crossbeam::mem::epoch::{self, Atomic};
use sponge::Sponge;

/// A lock-free, concurrent hash map.
// TODO: Make assumptions about `Hash` clear.
pub struct HashMap<K, V> {
    /// The root table of the hash map.
    table: table::Table<K, V>,
}

impl<K: Hash + Eq, V> HashMap<K, V> {
    /// Insert a key with a certain value into the map.
    pub fn insert(&self, key: K, val: V) -> Option<conc::Guard<V>> {
        self.table.insert(table::Pair {
            key: key,
            val: val,
        }, Sponge::new(&key))
    }

    /// Remove a key from the hash map.
    pub fn remove(&self, key: K) -> Option<conc::Guard<V>> {
        self.table.remove(key, Sponge::new(&key))
    }

    /// Apply a closure to every entry in the map.
    pub fn for_each<F: Fn(K, V)>(&self, f: F) {
        self.table.for_each(f);
    }

    /// Remove and apply a closure to every entry in the map.
    pub fn take_each<F: Fn(K, V)>(&self, f: F) {
        self.table.take_each(f);
    }

    /// Remove every entry from the map.
    pub fn clear(&self) {
        self.take_each(|_| ());
    }
}

impl<'a, K: Hash + Eq, V> Into<std::collections::HashMap<K, V>> for &'a HashMap<K, V> {
    fn into(self) -> std::collections::HashMap<K, V> {
        let mut hm = std::collections::HashMap::new();
        self.for_each(|key, val| {
            hm.insert(key, val);
        });

        hm
    }
}
