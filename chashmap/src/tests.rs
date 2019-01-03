// Copyright 2014-2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::thread;
use std::cell::RefCell;
use std::sync::Arc;
use CHashMap;

#[test]
fn spam_insert() {
    let m = Arc::new(CHashMap::new());
    let mut joins = Vec::new();

    for t in 0..10 {
        let m = m.clone();
        joins.push(thread::spawn(move || {
            for i in t * 1000..(t + 1) * 1000 {
                assert!(m.insert(i, !i).is_none());
                assert_eq!(m.insert(i, i).unwrap(), !i);
            }
        }));
    }

    for j in joins.drain(..) {
        j.join().unwrap();
    }

    for t in 0..5 {
        let m = m.clone();
        joins.push(thread::spawn(move || {
            for i in t * 2000..(t + 1) * 2000 {
                assert_eq!(*m.get(&i).unwrap(), i);
            }
        }));
    }

    for j in joins {
        j.join().unwrap();
    }
}

#[test]
fn spam_insert_new() {
    let m = Arc::new(CHashMap::new());
    let mut joins = Vec::new();

    for t in 0..10 {
        let m = m.clone();
        joins.push(thread::spawn(move || {
            for i in t * 1000..(t + 1) * 1000 {
                m.insert_new(i, i);
            }
        }));
    }

    for j in joins.drain(..) {
        j.join().unwrap();
    }

    for t in 0..5 {
        let m = m.clone();
        joins.push(thread::spawn(move || {
            for i in t * 2000..(t + 1) * 2000 {
                assert_eq!(*m.get(&i).unwrap(), i);
            }
        }));
    }

    for j in joins {
        j.join().unwrap();
    }
}

#[test]
fn spam_upsert() {
    let m = Arc::new(CHashMap::new());
    let mut joins = Vec::new();

    for t in 0..10 {
        let m = m.clone();
        joins.push(thread::spawn(move || {
            for i in t * 1000..(t + 1) * 1000 {
                m.upsert(i, || !i, |_| unreachable!());
                m.upsert(i, || unreachable!(), |x| *x = !*x);
            }
        }));
    }

    for j in joins.drain(..) {
        j.join().unwrap();
    }

    for t in 0..5 {
        let m = m.clone();
        joins.push(thread::spawn(move || {
            for i in t * 2000..(t + 1) * 2000 {
                assert_eq!(*m.get(&i).unwrap(), i);
            }
        }));
    }

    for j in joins {
        j.join().unwrap();
    }
}

#[test]
fn spam_alter() {
    let m = Arc::new(CHashMap::new());
    let mut joins = Vec::new();

    for t in 0..10 {
        let m = m.clone();
        joins.push(thread::spawn(move || {
            for i in t * 1000..(t + 1) * 1000 {
                m.alter(i, |x| {
                    assert!(x.is_none());
                    Some(!i)
                });
                m.alter(i, |x| {
                    assert_eq!(x, Some(!i));
                    Some(!x.unwrap())
                });
            }
        }));
    }

    for j in joins.drain(..) {
        j.join().unwrap();
    }

    for t in 0..5 {
        let m = m.clone();
        joins.push(thread::spawn(move || {
            for i in t * 2000..(t + 1) * 2000 {
                assert_eq!(*m.get(&i).unwrap(), i);
                m.alter(i, |_| None);
                assert!(m.get(&i).is_none());
            }
        }));
    }

    for j in joins {
        j.join().unwrap();
    }
}

#[test]
fn lock_compete() {
    let m = Arc::new(CHashMap::new());

    m.insert("hey", "nah");

    let k = m.clone();
    let a = thread::spawn(move || {
        *k.get_mut(&"hey").unwrap() = "hi";
    });
    let k = m.clone();
    let b = thread::spawn(move || {
        *k.get_mut(&"hey").unwrap() = "hi";
    });

    a.join().unwrap();
    b.join().unwrap();

    assert_eq!(*m.get(&"hey").unwrap(), "hi");
}

#[test]
fn simultanous_reserve() {
    let m = Arc::new(CHashMap::new());
    let mut joins = Vec::new();

    m.insert(1, 2);
    m.insert(3, 6);
    m.insert(8, 16);

    for _ in 0..10 {
        let m = m.clone();
        joins.push(thread::spawn(move || {
            m.reserve(1000);
        }));
    }

    for j in joins {
        j.join().unwrap();
    }

    assert_eq!(*m.get(&1).unwrap(), 2);
    assert_eq!(*m.get(&3).unwrap(), 6);
    assert_eq!(*m.get(&8).unwrap(), 16);
}

#[test]
fn create_capacity_zero() {
    let m = CHashMap::with_capacity(0);

    assert!(m.insert(1, 1).is_none());

    assert!(m.contains_key(&1));
    assert!(!m.contains_key(&0));
}

#[test]
fn insert() {
    let m = CHashMap::new();
    assert_eq!(m.len(), 0);
    assert!(m.insert(1, 2).is_none());
    assert_eq!(m.len(), 1);
    assert!(m.insert(2, 4).is_none());
    assert_eq!(m.len(), 2);
    assert_eq!(*m.get(&1).unwrap(), 2);
    assert_eq!(*m.get(&2).unwrap(), 4);
}

#[test]
fn upsert() {
    let m = CHashMap::new();
    assert_eq!(m.len(), 0);
    m.upsert(1, || 2, |_| unreachable!());
    assert_eq!(m.len(), 1);
    m.upsert(2, || 4, |_| unreachable!());
    assert_eq!(m.len(), 2);
    assert_eq!(*m.get(&1).unwrap(), 2);
    assert_eq!(*m.get(&2).unwrap(), 4);
}

#[test]
fn upsert_update() {
    let m = CHashMap::new();
    m.insert(1, 2);
    m.upsert(1, || unreachable!(), |x| *x += 2);
    m.insert(2, 3);
    m.upsert(2, || unreachable!(), |x| *x += 3);
    assert_eq!(*m.get(&1).unwrap(), 4);
    assert_eq!(*m.get(&2).unwrap(), 6);
}

#[test]
fn alter_string() {
    let m = CHashMap::new();
    assert_eq!(m.len(), 0);
    m.alter(1, |_| Some(String::new()));
    assert_eq!(m.len(), 1);
    m.alter(1, |x| {
        let mut x = x.unwrap();
        x.push('a');
        Some(x)
    });
    assert_eq!(m.len(), 1);
    assert_eq!(&*m.get(&1).unwrap(), "a");
}

#[test]
fn clear() {
    let m = CHashMap::new();
    assert!(m.insert(1, 2).is_none());
    assert!(m.insert(2, 4).is_none());
    assert_eq!(m.len(), 2);

    let om = m.clear();
    assert_eq!(om.len(), 2);
    assert_eq!(*om.get(&1).unwrap(), 2);
    assert_eq!(*om.get(&2).unwrap(), 4);

    assert!(m.is_empty());
    assert_eq!(m.len(), 0);

    assert_eq!(m.get(&1), None);
    assert_eq!(m.get(&2), None);
}

#[test]
fn clear_with_retain() {
    let m = CHashMap::new();
    assert!(m.insert(1, 2).is_none());
    assert!(m.insert(2, 4).is_none());
    assert_eq!(m.len(), 2);

    m.retain(|_, _| false);

    assert!(m.is_empty());
    assert_eq!(m.len(), 0);

    assert_eq!(m.get(&1), None);
    assert_eq!(m.get(&2), None);
}

#[test]
fn retain() {
    let m = CHashMap::new();
    m.insert(1, 8);
    m.insert(2, 9);
    m.insert(3, 4);
    m.insert(4, 7);
    m.insert(5, 2);
    m.insert(6, 5);
    m.insert(7, 2);
    m.insert(8, 3);

    m.retain(|key, val| key & 1 == 0 && val & 1 == 1);

    assert_eq!(m.len(), 4);

    for (key, val) in m {
        assert_eq!(key & 1, 0);
        assert_eq!(val & 1, 1);
    }
}

thread_local! { static DROP_VECTOR: RefCell<Vec<isize>> = RefCell::new(Vec::new()) }

#[derive(Hash, PartialEq, Eq)]
struct Dropable {
    k: usize
}

impl Dropable {
    fn new(k: usize) -> Dropable {
        DROP_VECTOR.with(|slot| {
            slot.borrow_mut()[k] += 1;
        });

        Dropable { k: k }
    }
}

impl Drop for Dropable {
    fn drop(&mut self) {
        DROP_VECTOR.with(|slot| {
            slot.borrow_mut()[self.k] -= 1;
        });
    }
}

impl Clone for Dropable {
    fn clone(&self) -> Dropable {
        Dropable::new(self.k)
    }
}

#[test]
fn drops() {
    DROP_VECTOR.with(|slot| {
        *slot.borrow_mut() = vec![0; 200];
    });

    {
        let m = CHashMap::new();

        DROP_VECTOR.with(|v| {
            for i in 0..200 {
                assert_eq!(v.borrow()[i], 0);
            }
        });

        for i in 0..100 {
            let d1 = Dropable::new(i);
            let d2 = Dropable::new(i+100);
            m.insert(d1, d2);
        }

        DROP_VECTOR.with(|v| {
            for i in 0..200 {
                assert_eq!(v.borrow()[i], 1);
            }
        });

        for i in 0..50 {
            let k = Dropable::new(i);
            let v = m.remove(&k);

            assert!(v.is_some());

            DROP_VECTOR.with(|v| {
                assert_eq!(v.borrow()[i], 1);
                assert_eq!(v.borrow()[i+100], 1);
            });
        }

        DROP_VECTOR.with(|v| {
            for i in 0..50 {
                assert_eq!(v.borrow()[i], 0);
                assert_eq!(v.borrow()[i+100], 0);
            }

            for i in 50..100 {
                assert_eq!(v.borrow()[i], 1);
                assert_eq!(v.borrow()[i+100], 1);
            }
        });
    }

    DROP_VECTOR.with(|v| {
        for i in 0..200 {
            assert_eq!(v.borrow()[i], 0);
        }
    });
}

#[test]
fn move_iter_drops() {
    DROP_VECTOR.with(|v| {
        *v.borrow_mut() = vec![0; 200];
    });

    let hm = {
        let hm = CHashMap::new();

        DROP_VECTOR.with(|v| {
            for i in 0..200 {
                assert_eq!(v.borrow()[i], 0);
            }
        });

        for i in 0..100 {
            let d1 = Dropable::new(i);
            let d2 = Dropable::new(i+100);
            hm.insert(d1, d2);
        }

        DROP_VECTOR.with(|v| {
            for i in 0..200 {
                assert_eq!(v.borrow()[i], 1);
            }
        });

        hm
    };

    // By the way, ensure that cloning doesn't screw up the dropping.
    drop(hm.clone());

    {
        let mut half = hm.into_iter().take(50);

        DROP_VECTOR.with(|v| {
            for i in 0..200 {
                assert_eq!(v.borrow()[i], 1);
            }
        });

        for _ in half.by_ref() {}

        DROP_VECTOR.with(|v| {
            let nk = (0..100).filter(|&i| {
                v.borrow()[i] == 1
            }).count();

            let nv = (0..100).filter(|&i| {
                v.borrow()[i+100] == 1
            }).count();

            assert_eq!(nk, 50);
            assert_eq!(nv, 50);
        });
    };

    DROP_VECTOR.with(|v| {
        for i in 0..200 {
            assert_eq!(v.borrow()[i], 0);
        }
    });
}

#[test]
fn empty_pop() {
    let m: CHashMap<isize, bool> = CHashMap::new();
    assert_eq!(m.remove(&0), None);
}

#[test]
fn lots_of_insertions() {
    let m = CHashMap::new();

    // Try this a few times to make sure we never screw up the hashmap's internal state.
    for _ in 0..10 {
        assert!(m.is_empty());

        for i in 1..1001 {
            assert!(m.insert(i, i).is_none());

            for j in 1..i+1 {
                let r = m.get(&j);
                assert_eq!(*r.unwrap(), j);
            }

            for j in i+1..1001 {
                let r = m.get(&j);
                assert_eq!(r, None);
            }
        }

        for i in 1001..2001 {
            assert!(!m.contains_key(&i));
        }

        // remove forwards
        for i in 1..1001 {
            assert!(m.remove(&i).is_some());

            for j in 1..i+1 {
                assert!(!m.contains_key(&j));
            }

            for j in i+1..1001 {
                assert!(m.contains_key(&j));
            }
        }

        for i in 1..1001 {
            assert!(!m.contains_key(&i));
        }

        for i in 1..1001 {
            assert!(m.insert(i, i).is_none());
        }

        // remove backwards
        for i in (1..1001).rev() {
            assert!(m.remove(&i).is_some());

            for j in i..1001 {
                assert!(!m.contains_key(&j));
            }

            for j in 1..i {
                assert!(m.contains_key(&j));
            }
        }
    }
}

#[test]
fn find_mut() {
    let m = CHashMap::new();
    assert!(m.insert(1, 12).is_none());
    assert!(m.insert(2, 8).is_none());
    assert!(m.insert(5, 14).is_none());
    let new = 100;
    match m.get_mut(&5) {
        None => panic!(), Some(mut x) => *x = new
    }
    assert_eq!(*m.get(&5).unwrap(), new);
}

#[test]
fn insert_overwrite() {
    let m = CHashMap::new();
    assert_eq!(m.len(), 0);
    assert!(m.insert(1, 2).is_none());
    assert_eq!(m.len(), 1);
    assert_eq!(*m.get(&1).unwrap(), 2);
    assert_eq!(m.len(), 1);
    assert!(!m.insert(1, 3).is_none());
    assert_eq!(m.len(), 1);
    assert_eq!(*m.get(&1).unwrap(), 3);
}

#[test]
fn insert_conflicts() {
    let m = CHashMap::with_capacity(4);
    assert!(m.insert(1, 2).is_none());
    assert!(m.insert(5, 3).is_none());
    assert!(m.insert(9, 4).is_none());
    assert_eq!(*m.get(&9).unwrap(), 4);
    assert_eq!(*m.get(&5).unwrap(), 3);
    assert_eq!(*m.get(&1).unwrap(), 2);
}

#[test]
fn conflict_remove() {
    let m = CHashMap::with_capacity(4);
    assert!(m.insert(1, 2).is_none());
    assert_eq!(*m.get(&1).unwrap(), 2);
    assert!(m.insert(5, 3).is_none());
    assert_eq!(*m.get(&1).unwrap(), 2);
    assert_eq!(*m.get(&5).unwrap(), 3);
    assert!(m.insert(9, 4).is_none());
    assert_eq!(*m.get(&1).unwrap(), 2);
    assert_eq!(*m.get(&5).unwrap(), 3);
    assert_eq!(*m.get(&9).unwrap(), 4);
    assert!(m.remove(&1).is_some());
    assert_eq!(*m.get(&9).unwrap(), 4);
    assert_eq!(*m.get(&5).unwrap(), 3);
}

#[test]
fn is_empty() {
    let m = CHashMap::with_capacity(4);
    assert!(m.insert(1, 2).is_none());
    assert!(!m.is_empty());
    assert!(m.remove(&1).is_some());
    assert!(m.is_empty());
}

#[test]
fn pop() {
    let m = CHashMap::new();
    m.insert(1, 2);
    assert_eq!(m.remove(&1), Some(2));
    assert_eq!(m.remove(&1), None);
}

#[test]
fn find() {
    let m = CHashMap::new();
    assert!(m.get(&1).is_none());
    m.insert(1, 2);
    let lock = m.get(&1);
    match lock {
        None => panic!(),
        Some(v) => assert_eq!(*v, 2)
    }
}

#[test]
fn reserve_shrink_to_fit() {
    let m = CHashMap::new();
    m.insert(0, 0);
    m.remove(&0);
    assert!(m.capacity() >= m.len());
    for i in 0..128 {
        m.insert(i, i);
    }
    m.reserve(256);

    let usable_cap = m.capacity();
    for i in 128..(128 + 256) {
        m.insert(i, i);
        assert_eq!(m.capacity(), usable_cap);
    }

    for i in 100..(128 + 256) {
        assert_eq!(m.remove(&i), Some(i));
    }
    m.shrink_to_fit();

    assert_eq!(m.len(), 100);
    assert!(!m.is_empty());
    assert!(m.capacity() >= m.len());

    for i in 0..100 {
        assert_eq!(m.remove(&i), Some(i));
    }
    m.shrink_to_fit();
    m.insert(0, 0);

    assert_eq!(m.len(), 1);
    assert!(m.capacity() >= m.len());
    assert_eq!(m.remove(&0), Some(0));
}

#[test]
fn from_iter() {
    let xs = [(1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6)];

    let map: CHashMap<_, _> = xs.iter().cloned().collect();

    for &(k, v) in &xs {
        assert_eq!(*map.get(&k).unwrap(), v);
    }
}

#[test]
fn capacity_not_less_than_len() {
    let a = CHashMap::new();
    let mut item = 0;

    for _ in 0..116 {
        a.insert(item, 0);
        item += 1;
    }

    assert!(a.capacity() > a.len());

    let free = a.capacity() - a.len();
    for _ in 0..free {
        a.insert(item, 0);
        item += 1;
    }

    assert_eq!(a.len(), a.capacity());

    // Insert at capacity should cause allocation.
    a.insert(item, 0);
    assert!(a.capacity() > a.len());
}

#[test]
fn insert_into_map_full_of_free_buckets() {
    let m = CHashMap::with_capacity(1);
    for i in 0..100 {
        m.insert(i, 0);
        m.remove(&i);
    }
}

#[test]
fn lookup_borrowed() {
    let m = CHashMap::with_capacity(1);
    m.insert("v".to_owned(), "value");
    m.get("v").unwrap();
}
