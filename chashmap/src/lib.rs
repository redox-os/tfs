extern crate parking_lot;
extern crate owning_ref;

use parking_lot::{RwLock, RwLockWriteGuard, RwLockReadGuard};
use owning_ref::{OwningHandle, OwningRef};
use std::sync::atomic::{self, AtomicUsize};
use std::hash::{self, Hash, Hasher};
use std::{mem, ops};

const ORDERING: atomic::Ordering = atomic::Ordering::SeqCst;
const LENGTH_MULTIPLIER: usize = 4;
const MAX_LOAD_FACTOR_NUM: usize = 100 - 15;
const MAX_LOAD_FACTOR_DENOM: usize = 100;

fn hash<K: Hash>(key: K) -> usize {
    let mut h = hash::SipHasher::new();
    key.hash(&mut h);
    h.finish() as usize
}

enum Bucket<K, V> {
    Contains(K, V),
    Removed,
    Empty,
}

impl<K, V> Bucket<K, V> {
    fn remove(&mut self) -> Bucket<K, V> {
        mem::replace(self, Bucket::Removed)
    }

    fn value(self) -> Option<V> {
        if let Bucket::Contains(_, val) = self {
            Some(val)
        } else { None }
    }
}

struct Table<K, V> {
    buckets: Vec<RwLock<Bucket<K, V>>>,
}

impl<K: PartialEq + Hash, V> Table<K, V> {
    fn scan<F>(&self, key: &K, matches: F) -> RwLockReadGuard<Bucket<K, V>>
        where F: Fn(&Bucket<K, V>) -> bool {
        let hash = hash(key);
        let len = self.buckets.len();

        for i in 0.. {
            let lock = self.buckets[(hash + i) % len].read();

            if matches(&lock) {
                return lock;
            }
        }

        unreachable!();
    }

    fn scan_mut<F>(&self, key: &K, matches: F) -> RwLockWriteGuard<Bucket<K, V>>
        where F: Fn(&Bucket<K, V>) -> bool {
        let hash = hash(key);
        let len = self.buckets.len();

        for i in 0.. {
            let lock = self.buckets[(hash + i) % len].write();

            if matches(&lock) {
                return lock;
            }
        }

        unreachable!();
    }

    fn scan_mut_no_lock<F>(&mut self, key: &K, matches: F) -> &mut Bucket<K, V>
        where F: Fn(&Bucket<K, V>) -> bool {
        let hash = hash(key);
        let len = self.buckets.len();

        for i in 0.. {
            let bucket = self.buckets[(hash + i) % len].get_mut();

            if matches(&bucket) {
                return bucket;
            }
        }

        unreachable!();
    }

    fn lookup(&self, key: &K) -> RwLockReadGuard<Bucket<K, V>> {
        self.scan(key, |x| match *x {
            Bucket::Contains(ref candidate_key, _) if key == candidate_key => true,
            Bucket::Empty => true,
            _ => false,
        })
    }

    fn lookup_mut(&self, key: &K) -> RwLockWriteGuard<Bucket<K, V>> {
        self.scan_mut(key, |x| match *x {
            Bucket::Contains(ref candidate_key, _) if key == candidate_key => true,
            Bucket::Empty => true,
            _ => false,
        })
    }

    fn find_free(&self, key: &K) -> RwLockWriteGuard<Bucket<K, V>> {
        self.scan_mut(key, |x| match *x {
            // Halt if and only if the bucket is free.
            Bucket::Removed | Bucket::Empty => true,
            _ => false,
        })
    }

    fn fill(&mut self, table: Vec<RwLock<Bucket<K, V>>>) {
        for mut i in table {
            if let Bucket::Contains(key, val) = i.get_mut().remove() {
                let mut bucket = self.scan_mut_no_lock(&key, |x| match *x {
                    Bucket::Removed | Bucket::Empty => true,
                    _ => false,
                });

                *bucket = Bucket::Contains(key, val);
            }
        }
    }
}

pub struct ReadGuard<'a, K: 'a, V: 'a> {
    inner: OwningRef<OwningHandle<RwLockReadGuard<'a, Table<K, V>>, RwLockReadGuard<'a, Bucket<K, V>>>, V>,
}

impl<'a, K, V> ops::Deref for ReadGuard<'a, K, V> {
    type Target = V;

    fn deref(&self) -> &V {
        &self.inner
    }
}

pub struct WriteGuard<'a, K: 'a, V: 'a> {
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

pub struct CHashMap<K, V> {
    total: AtomicUsize,
    table: RwLock<Table<K, V>>,
}

impl<K: PartialEq + Hash, V> CHashMap<K, V> {
    pub fn get(&self, key: &K) -> Option<ReadGuard<K, V>> {
        if let Ok(inner) = OwningRef::new(OwningHandle::new(self.table.read(), |x| unsafe { &*x }.lookup(key)))
            .try_map(|x| if let Bucket::Contains(_, ref val) = *x {
            Ok(val)
        } else {
            Err(())
        }) {
            Some(ReadGuard {
                inner: inner,
            })
        } else { None }
    }

    pub fn get_mut(&self, key: &K) -> Option<WriteGuard<K, V>> {
        if let Ok(inner) = OwningHandle::try_new(OwningHandle::new(self.table.read(), |x| unsafe { &*x }.lookup_mut(key)), |x| if let &mut Bucket::Contains(_, ref mut val) = unsafe { &mut *(x as *mut Bucket<K, V>) } {
            Ok(val)
        } else {
            Err(())
        }) {
            Some(WriteGuard {
                inner: inner,
            })
        } else { None }
    }

    pub fn insert(&self, key: K, val: V) {
        let lock = self.expand();
        let mut bucket = lock.find_free(&key);

        *bucket = Bucket::Contains(key, val);
    }

    pub fn replace(&self, key: K, val: V) -> Option<V> {
        let lock = self.expand();
        let mut bucket = lock.lookup_mut(&key);

        mem::replace(&mut *bucket, Bucket::Contains(key, val)).value()
    }

    pub fn remove(&self, key: &K) -> Option<V> {
        let lock = self.table.read();
        self.total.fetch_sub(1, ORDERING);

        let mut bucket = lock.lookup_mut(&key);
        bucket.remove().value()
    }

    pub fn reserve(&self, len: usize) {
        if self.total.load(ORDERING) < len {
            let mut lock = self.table.write();
            let vec = mem::replace(&mut lock.buckets, Vec::with_capacity(len * LENGTH_MULTIPLIER));
            lock.fill(vec);
        }
    }

    pub fn shrink_to_fit(&self) {
        let mut lock = self.table.write();
        let vec = mem::replace(&mut lock.buckets, Vec::with_capacity(self.total.load(ORDERING) * LENGTH_MULTIPLIER));
        lock.fill(vec);
    }

    fn expand(&self) -> RwLockReadGuard<Table<K, V>> {
        let lock = self.table.read();
        let total = self.total.fetch_add(1, ORDERING);

        // Extend if necessary. We multiply by some constant to adjust our load factor.
        if total * MAX_LOAD_FACTOR_DENOM >= lock.buckets.len() * MAX_LOAD_FACTOR_NUM {
            drop(lock);
            self.reserve(total + 1);
            self.table.read()
        } else {
            lock
        }
    }
}
