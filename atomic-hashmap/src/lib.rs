mod sponge;
mod table;

pub struct HashMap<K, V> {
    table: table::Table<K, V>,
}

impl<K: Hash, V> HashMap<K, V> {
    pub fn insert(&self, key: K, val: V) -> Option<V> {
        self.table.insert(Pair {
            key: key,
            val: val,
        }, Sponge::new(&key))
    }

    pub fn remove(&self, key: K, sponge: Sponge) -> Option<V> {
        self.table.remove(key, Sponge::new(&key))
    }

    pub fn for_each<F: Fn(K, V)>(&self, f: F) {
        self.table.for_each(f);
    }

    pub fn take_each<F: Fn(K, V)>(&self, f: F) {
        self.table.take_each(f);
    }

    pub fn clear(&self) {
        self.take(|| ());
    }
}

impl<'a, K, V> Into<std::collections::HashMap<K, V>> for &'a HashMap<K, V> {
    fn into(self) -> std::collections::HashMap<K, V> {
        let mut hm = std::collections::HashMap::new();
        self.for_each(|key, val| hm.insert(key, val));

        hm
    }
}
