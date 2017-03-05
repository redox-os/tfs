trait Atomic {
    fn load(&self) -> Self;
    fn store(&self, new: Self);
    fn swap(&self, new: Self) -> Self;
    fn cas(&self, old: Self, new: Self) -> Result<(), Self>;
}

trait Key: Atomic {
    const EMPTY: Self;
    const HEADSTONE: Self;
    const BLOCKED: Self;

    fn store_if_free(&self, new: Self) -> Result<(), Self> {
        // If it went changed in between here, it wouldn't be a problem, since it will never go
        // from headstone to empty, so if it's set to blocked, its failure will never leave a gap
        // in the cluster (empty key).
        self.cas(EMPTY, new).or_else(|_| self.cas(HEADSTONE, new))
    }
}

enum TableResult<T> {
    Ok(T),
    ReallocationWanted(T),
    ReallocationNeeded,
}

impl Reallocation {
    fn succeeded(self) -> bool {
        match self {
            Reallocation::Needed => false,
            Reallocation::Needed => false,
        }
    }
}

struct Bucket<K, V> {
    key: K,
    val: V,
}

struct RawTable<K, V> {
    buckets: Vec<(K, V)>,
    /// An upper-bound on the number of valid keys in the table.
    keys: AtomicUsize,
}

impl<K: Key + Hash, V: Atomic> RawTable<K, V> {
    fn hash(&self, key: &K) -> usize {
        unimplemented!();
    }

    fn remove(&self, key: K) -> bool {
        let hash = self.hash(&key);

        for hash in hash.. {
            let bucket = &self.buckets[hash % self.buckets.len()];

            if let Err(non_matching_key) = bucket.key.cas(key, K::HEADSTONE) {
                if non_matching_key == K::EMPTY {
                    // The cluster is delimited by an empty bucket, so we know there is no matching
                    // key.
                    return false;
                }
            } else {
                self.decrement_keys();

                return true;
            }
        }
    }

    fn take(&self, key: K) -> Option<V> {
        let hash = self.hash(&key);

        for hash in hash.. {
            let bucket = &self.buckets[hash % self.buckets.len()];

            if let Err(non_matching_key) = bucket.key.cas(key, K::BLOCKED) {
                if non_matching_key == K::EMPTY {
                    // The cluster is delimited by an empty bucket, so we know there is no matching
                    // key.
                    return None;
                }
            } else {
                let ret = bucket.val.load();
                self.decrement_keys();
                bucket.key.store(K::HEADSTONE);
                return Some(ret);
            }
        }
    }

    /// Insert a **new** entry into the table.
    ///
    /// This inserts value `val` at key `key`. If an entry with key `key` already exists, the entry
    /// will retain its value and not be updated.
    fn insert_new(&self, key: K, val: V) -> TableResult<()> {
        let hash = self.hash(&key);

        let new_keys = self.increment_keys();
        if new_keys > self.buckets.len() {
            return TableResult::ReallocationNeeded;
        }

        for hash in hash.. {
            let bucket = &self.buckets[hash % self.buckets.len()];

            if bucket.key.load() == key {
                self.decrement_keys();
                break;
            }

            // TODO: Remember deduplicating when reallocating, since duplicates can happen if a key
            //       which later becomes A is set to blocked, while another insertion of key A
            //       happens.
            if bucket.key.store_if_free(K::BLOCKED).is_ok() {
                bucket.val.store(val);
                bucket.key.store(key);
                break;
            }
        }

        if self.should_realloc(new_keys) {
            TableResult::ReallocationWanted(())
        } else {
            TableResult::Ok(())
        }
    }

    fn replace(&self, key: K, val: V) -> TableResult<Option<V>> {
        let hash = self.hash(&key);

        let new_keys = self.increment_keys();
        if new_keys > self.buckets.len() {
            return TableResult::ReallocationNeeded;
        }

        for hash in hash.. {
            let bucket = &self.buckets[hash % self.buckets.len()];

            if bucket.key.load() == key {
                self.decrement_keys();
                break;
            }

            if bucket.key.store_if_free(K::BLOCKED).is_ok() {
                bucket.val.store(val);
                bucket.key.store(key);
                break;
            }
        }

        if self.should_realloc(new_keys) {
            TableResult::ReallocationWanted(())
        } else {
            TableResult::Ok(())
        }
    }
}

struct RawMap<K, V> {
    new: epoch::Atomic<RawTable<K, V>>,
    old: epoch::Atomic<RawTable<K, V>>,
}
