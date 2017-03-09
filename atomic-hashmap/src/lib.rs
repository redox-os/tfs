extern crate crossbeam;

mod sponge;
mod table;

use std::hash::Hash;
use sponge::Sponge;

pub struct HashMap<K, V> {
    table: table::Table<K, V>,
}

impl<K: Hash + Eq, V> HashMap<K, V> {
    pub fn insert(&self, key: K, val: V) -> Option<epoch::Pinned<V>> {
        let guard = epoch::pin();

        self.table.insert(table::Pair {
            key: key,
            val: val,
        }, Sponge::new(&key), guard).into_pinned(guard)
    }

    pub fn remove(&self, key: K, sponge: Sponge) -> Option<epoch::Pinned<V>> {
        let guard = epoch::pin();

        self.table.remove(key, Sponge::new(&key), guard).into_pinned(guard)
    }

    pub fn for_each<F: Fn(K, V)>(&self, f: F) {
        let guard = epoch::pin();
        self.table.for_each(f, guard);
    }

    pub fn take_each<F: Fn(K, V)>(&self, f: F) {
        let guard = epoch::pin();
        self.table.take_each(f, guard);
    }

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
