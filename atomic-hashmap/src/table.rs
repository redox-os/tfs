use std::sync::atomic;
use std::hash::Hash;
use crossbeam::mem::epoch::{Atomic, Owned, Shared};
use sponge::Sponge;

const ORDERING: atomic::Ordering = atomic::Ordering::Relaxed;

pub struct Pair<K, V> {
    key: K,
    val: V,
}

#[derive(Default)]
pub struct Table<K, V>  {
    table: [Atomic<Node<K, V>>; 256],
}

impl<K, V> Table<K, V> {
    fn two_entries(pair_a: Pair<K, V>, sponge_a: Sponge, pair_b: Pair<K, V>, sponge_b: Sponge)
        -> Table<K, V> {
        // Start with an empty table.
        let mut table = Table::default();

        // Squeeze the two sponges.
        let pos_a = sponge_a.squeeze();
        let pos_b = sponge_b.squeeze();

        if pos_a != pos_b {
            // The two position did not collide, so we can insert the two pairs at the respective
            // positions
            table[pos_a as usize] = Atomic::new(Some(Owned::new(Node::Leaf(pair_a))));
            table[pos_b as usize] = Atomic::new(Some(Owned::new(Node::Leaf(pair_b))));
        } else {
            // The two positions from the sponge matched, so we must place another branch.
            table[pos_a as usize] = Atomic::new(Some(Owned::new(Node::Branch(
                Table::two_entries(pair_a, sponge_a, pair_b, sponge_b)
            ))));
        }
    }

    pub fn get(&self, key: &K, sponge: Sponge) -> Option<Shared<V>> {
        // Load the entry and handle the respective cases.
        match self.table[sponge.squeeze()].load(ORDERING) {
            // The entry was a leaf and the keys match, so we can return the entry's value.
            Some(Node::Leaf(Pair { found_key, found_val })) if key == found_key => Some(found_val),
            // The entry is a branch with another table, so we recurse and look up in said
            // subtable.
            Some(Node::Branch(table)) => table.get(key, sponge),
            // The entry is either a leaf but doesn't match, or is a null pointer, meaning there is
            // no entry with the key.
            Some(Node::Leaf(_)) | None => None,
        }
    }

    pub fn remove(&self, key: &K, sponge: Sponge) -> Option<V> {
        // We squeeze the sponge to get the right entry of our table, in which we will potentially
        // remove the key.
        self.table[sponge.squeeze()].remove(key, sponge);
    }

    pub fn insert(&self, pair: Pair<K, V>, sponge: Sponge) -> Option<V> {
        // We squeeze the sponge to get the right entry of our table, in which we will insert our
        // key-value pair.
        self.table[sponge.squeeze()].load(ORDERING).insert(pair, sponge)
    }

    pub fn for_each<F: Fn(K, V)>(&self, f: F) {
        for i in self.table {
            i.for_each(f);
        }
    }

    pub fn take_each<F: Fn(K, V)>(&self, f: F) {
        for i in self.table {
            i.take_each(f);
        }
    }
}

enum Node<K, V> {
    Leaf(Pair<K, V>),
    Branch(Table<K, V>),
}

impl<K, V> Atomic<Node<K, V>> {
    /// Insert a key-value pair into the node without squeezing the sponge.
    fn insert(&self, pair: Pair<K, V>, sponge: Sponge) -> Option<V> {
        // If the entry was empty, we will set it to a leaf with our key-value pair. If not, we
        // will handle the respective cases manually.
        match self.cas(None, Some(Owned::new(Node::Leaf(pair))), ORDERING) {
            // We successfully set an empty entry to the new key-value pair. This of course implies
            // that the key didn't exist at the time.
            Ok(()) => None,
            // There is a branch table. Insert the key-value pair into it.
            Err(Some(Node::Branch(table))) => table.insert(pair, sponge),
            // The key exists, so we can simply update the value.
            Err(Some(Node::Leaf(found_pair))) if found_pair.key == pair.key
                // The reason we use CAS here is that the key could have been removed or updated
                // after we read it initially. If so, we won't update it for the reason that it was
                // logically inserted (`insert` was called) before it being removed or updated.
                // Hence the other version is used, and we don't touch it.
                => match self.cas(Some(Node::Leaf(found_pair)), Some(Node::Leaf(pair)), ORDERING) {
                    // Everything went well and the leaf was updated.
                    Ok(()) => Some(found_pair.val),
                    // Another node was inserted here, meaning that the table is simply extended,
                    // and the insertion is still potentially valid, so we try to insert in the
                    // inner table.
                    Err(Some(Node::Branch(table))) => table.insert(pair, sponge),
                    // The node was either modified or removed (and maybe replaced by another
                    // node). The insertion was hence invalidated, and we simply pretend we
                    // replaced the leaf we read earlier (even though that was another thread's
                    // work), which we did "logically" (at the point the leaf was read, it had
                    // matching keys).
                    _ => Some(found_pair.val),
                },
            // Another key exists at the position, so we need to extend the table with a branch,
            // containing both entries.
            Err(Some(Node::Leaf(mut old_pair))) => {
                // Create a table that contains both the key-value pair we're inserting and the one
                // on the place, where we want to insert.
                let new_table = Table::two_entries(pair, sponge, old_pair, {
                    // Generate the sponge of the old pair's key.
                    let mut old_sponge = Sponge::new(&old_pair.key);
                    // Truncate the sponge, so it is at the point, where we are right now, and the
                    // collision is happening.
                    old_sponge.matching(&sponge);

                    old_sponge
                });
                // We try to update the current entry to our table. The reason we use CAS is that
                // we want to ensure that the entry was not changed in the meantime, so we compare
                // to `old_pair`, which must be a leaf with the old key-value pair, as the epoch
                // system ensures that it doesn't change while we have the reference (therefore
                // there is no ABA problem here). So in essence, we check that our value is still
                // the same as the original, and if it is we update it. If not, we must handle the
                // new value, which could be anything else (e.g. another thread could have extended
                // the leaf too because it is inserting the same pair).
                match self.cas(old_pair, Some(Owned::new(Node::Branch(new_table))), ORDERING) {
                    // Our update went smooth, and we have extended the leaf to a branch,
                    // meaning that there now is a subtable containing the two key-value pairs.
                    Ok(()) => None,
                    // The update failed. Another thread extended the table, so we will simply use
                    // the table from the new leaf that the other thread created.
                    Err(Some(Node::Branch(table))) => table.insert(pair, sponge),
                    // The node was either updated or removed in the meantime (i.e. before we
                    // logically inserted/`insert` was called), hence we assume this has affected
                    // the logical insertion, which therefore means that it shouldn't be written as
                    // it was overwritten. As a result, we simply return `None`, marking that no
                    // value was replaced.
                    _ => None,
                }
            },
            // It is not possible to get `Err(None)` as that was the value we are CAS-ing against.
            Err(None) => unreachable!(),
        }
    }

    fn remove(&self, key: &K, sponge: Sponge) -> Option<V> {
        // Load the node and handle its cases.
        match self.load(ORDERING) {
            // There is a branch, so we must remove the key in the sub-table.
            Some(Node::Branch(table)) => table.remove(key, sponge),
            // There was a node with the key, which we will try to remove. We use CAS in order to
            // make sure that it is the same node as the one we read (`self`), otherwise we might
            // remove a wrong node.
            Some(Node::Leaf(Pair { key: found_key, val })) if found_key == key
                => match self.cas(Some(self), None, ORDERING) {
                // Removing the node succeeded: It wasn't changed in the meantime.
                Ok(()) => Some(val),
                Err(Some(Node::Branch(table))) => table.remove(key, sponge),
                // The node was removed or updated by another thread, so our removal is "logically"
                // done, as it was overruled by another thread in the meantime, either by insertion
                // or removal of the same node. We return the value it had at time of the logical
                // removal (i.e. when `remove` was called), as the function acts as if it removed
                // the node.
                _ => Some(val),
            },
            // A node with a non-matching key was found. Hence, we have nothing to remove.
            Some(Node::Leaf(..)) => None,
            // No node was found; nothing to remove.
            None => None,
        }
    }

    fn for_each<F: Fn(K, V)>(&self, f: F) {
        match self.load(ORDERING) {
            Some(Node::Leaf(Pair { key, val })) => f(key, val),
            Some(Node::Branch(table)) => table.for_each(f),
        }
    }

    fn take_each<F: Fn(K, V)>(&self, f: F) {
        match self.load(ORDERING) {
            Some(Node::Leaf(Pair { key, val })) => f(key, val),
            Some(Node::Branch(table)) => table.take_each(f),
        }
    }
}
