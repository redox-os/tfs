//! The internal table structure.

use std::sync::atomic;
use std::hash::Hash;
use sponge::Sponge;
use conc;

/// A key-value pair.
pub struct Pair<K, V> {
    // The key.
    key: K,
    // The value.
    val: V,
}

/// A node in the tree.
enum Node<K, V> {
    /// A leaf containing a key-value pair.
    Leaf(Pair<K, V>),
    /// A branch to a subtable.
    Branch(Table<K, V>),
}

/// A table.
///
/// Tables are nothing but an array of buckets, being represented by atomic pointers. It can be
/// viewed as a lower-level API of the hash map itself.
#[derive(Default)]
pub struct Table<K, V>  {
    /// The buckets in the table.
    buckets: [conc::Atomic<Node<K, V>>; 256],
}

impl<K: Hash + Eq, V> Table<K, V> {
    /// Create a table containing two particular entries.
    ///
    /// This takes two key-value pairs, `pair_a` and `pair_b`, and their respective sponges, and
    /// creates a table containing both pairs.
    fn two_entries(
        pair_a: Pair<K, V>,
        sponge_a: Sponge,
        pair_b: Pair<K, V>,
        sponge_b: Sponge,
    ) -> Table<K, V> {
        // Start with an empty table.
        let mut table = Table::default();

        // Squeeze the two sponges.
        let pos_a = sponge_a.squeeze();
        let pos_b = sponge_b.squeeze();

        if pos_a != pos_b {
            // The two position did not collide, so we can insert the two pairs at the respective
            // positions
            table[pos_a as usize] = conc::Atomic::new(Some(Box::new(Node::Leaf(pair_a))));
            table[pos_b as usize] = conc::Atomic::new(Some(Box::new(Node::Leaf(pair_b))));
        } else {
            // The two positions from the sponge matched, so we must place another branch.
            table[pos_a as usize] = conc::Atomic::new(Some(Box::new(Node::Branch(
                Table::two_entries(pair_a, sponge_a, pair_b, sponge_b)
            ))));
        }

        table
    }

    /// Get the value associated with some key, given its sponge.
    pub fn get(&self, key: &K, sponge: Sponge) -> Option<conc::Guard<V>> {
        // Load the bucket and handle the respective cases.
        self.buckets[sponge.squeeze() as usize].load(atomic::Ordering::Acquire)
            .and_then(|node| node.map(|node| match node {
            // The bucket was a leaf and the keys match, so we can return the bucket's value.
            Node::Leaf(Pair { found_key, found_val }) if key == found_key => Some(found_val),
            // The bucket is a branch with another table, so we recurse and look up in said
            // sub-table.
            Node::Branch(table) => table.get(key, sponge),
            // The bucket is either a leaf but doesn't match, or is a null pointer, meaning there
            // is no bucket with the key.
            Node::Leaf(_) => None,
        }))
    }

    /// Insert a key-value pair into the table, given its sponge.
    pub fn insert(&self, pair: Pair<K, V>, sponge: Sponge) -> Option<V> {
        // We squeeze the sponge to get the right bucket of our table, in which we will insert our
        // key-value pair.
        let bucket = self.buckets[sponge.squeeze() as usize];

        // We use CAS to place the leaf if and only if the bucket is empty. Otherwise, we must
        // handle the respective cases.
        let mut node = match bucket.compare_and_swap(
            None,
            Some(Box::new(Node::Leaf(pair))),
            atomic::Ordering::Release
        ) {
            // The CAS succeeded, meaning that the key wasn't already in the structure, hence we
            // return `None`.
            Ok(()) => return None,
            // The CAS failed, so we handle the actual node in the loop below.
            Err(actual, _) => actual,
        };

        // To avoid the ABA problem, this loop is unfortunately necessary, but keep in mind that it
        // will rarely run more than one iteration, nor is it mutually exclusive in any way, it
        // just ensures that the bucket doesn't change in the meantime in certain specific cases.
        loop {
            // Handle the cases of the read snapshot.
            match node {
                // There is a branch table. Insert the key-value pair into it.
                Some(Node::Branch(table)) => return table.insert(pair, sponge),
                // The key exists, so we can simply update the value.
                Some(Node::Leaf(found_pair)) if found_pair.key == pair.key
                    // The reason we use CAS here is that the key could have been removed or
                    // updated after we read it initially. If so, we won't update it for the reason
                    // that it potentially would invalidate unrelated keys.
                    => match bucket.compare_and_swap(
                        Some(Node::Leaf(found_pair)),
                        Some(Node::Leaf(pair)),
                        atomic::Ordering::Release
                    ) {
                        // Everything went well and the leaf was updated.
                        Ok(()) => return Some(found_pair.val),
                        // The CAS failed, so we handle the actual node in the next loop iteration.
                        Err((actual, _)) => node = actual,
                    },
                // Another key exists at the position, so we need to extend the table with a
                // branch, containing both entries.
                Some(Node::Leaf(mut old_pair)) => {
                    // Create a table that contains both the key-value pair we're inserting and the
                    // one on the place, where we want to insert.
                    let new_table = Table::two_entries(pair, sponge, old_pair, {
                        // Generate the sponge of the old pair's key.
                        let mut old_sponge = Sponge::new(&old_pair.key);
                        // Truncate the sponge, so it is at the point, where we are right now, and
                        // the collision is happening.
                        old_sponge.matching(&sponge);

                        old_sponge
                    });
                    // We try to update the current bucket to our table. The reason we use CAS is
                    // that we want to ensure that the bucket was not changed in the meantime, so
                    // we compare to `old_pair`, which must be a leaf with the old key-value pair,
                    // as the CMR system ensures that it doesn't change while we have the reference
                    // (therefore there is no ABA problem here). So in essence, we check that our
                    // value is still the same as the original, and if it is we update it. If not,
                    // we must handle the new value, which could be anything else (e.g. another
                    // thread could have extended the leaf too because it is inserting the same
                    // pair).
                    match bucket.compare_and_swap(
                        old_pair,
                        Some(Box::new(Node::Branch(new_table))),
                        atomic::Ordering::Release
                    ) {
                        // The CAS succeeded, meaning that the old non-matching pair was replaced
                        // by a branch, and given that the old pair wasn't matching, the key wasn't
                        // already in the structure, so we return `None` to mark this.
                        Ok(()) => return None,
                        // The CAS failed, so we handle the actual node in the next loop iteration.
                        Err((actual, _)) => node = actual,
                    };
                },
                // As something clearly changed between the initial CAS (which attempted to swap an
                // empty bucket) and the most recent CAS, we must re-do the thing in order to
                // ensure correctness (so we re-do the CAS which was done in the start of the
                // function). Otherwise, the insertion might end up lost.
                None => {
                    match bucket.compare_and_swap(
                        None,
                        Some(Box::new(Node::Leaf(pair))),
                        atomic::Ordering::Release
                    ) {
                        // The CAS succeeded, meaning that the key wasn't already in the structure,
                        // hence we return `None`.
                        Ok(()) => return None,
                        // The CAS failed, so we handle the actual node in the next loop iteration.
                        Err(actual, _) => node = actual,
                    };
                },
            };
        }
    }

    /// Remove a key from the table, given its sponge.
    pub fn remove(
        &self,
        key: &K,
        sponge: Sponge,
    ) -> Option<conc::Guard<V>> {
        // We squeeze the sponge to get the right bucket of our table, in which we will potentially
        // remove the key.
        let bucket = self.buckets[sponge.squeeze() as usize];

        // Load the node.
        let mut node = bucket.load(atomic::Ordering::Acquire);

        // To avoid the ABA problem, this loop is unfortunately necessary, but keep in mind that it
        // will rarely run more than one iteration, nor is it mutually exclusive in any way, it
        // just ensures that the bucket doesn't change in the meantime in certain specific cases.
        loop {
            // Handle the respective cases.
            match node {
                // The read node was empty, so there's nothing to remove.
                None => return None,
                // There is a branch, so we must remove the key in the sub-table.
                Some(Node::Branch(table)) => return table.remove(key, sponge),
                // There was a node with the key, which we will try to remove. We use CAS in order
                // to make sure that it is the same node as the one we read (`bucket`), otherwise
                // we might remove a wrong node.
                Some(Node::Leaf(Pair { key: ref found_key, val })) if found_key == key
                    => match bucket.compare_and_swap(Some(bucket), None, atomic::Ordering::Release) {
                    // Removing the node succeeded: It wasn't changed in the meantime.
                    Ok(()) => return Some(val),
                    // The CAS failed, meaning that in the meantime, the node was changed, so we
                    // update the node variable and redo the loop to handle the new case.
                    Err(actual, _) => node = actual,
                },
                // A node with a non-matching key was found. Hence, we have nothing to remove.
                Some(Node::Leaf(..)) => return None,
            }
        }
    }

    /// Run a closure on every key-value pair in the table.
    pub fn for_each<F: Fn(K, V)>(&self, f: F) {
        // Go over every bucket in the table.
        for i in self.buckets {
            // Load the bucket from the table.
            match i.load(atomic::Ordering::Acquire) {
				// There is a leaf; we simply apply the function to the inner value.
                Some(Node::Leaf(Pair { key, val })) => f(key, val),
				// There is a branch; hence we must recurse with the call.
                Some(Node::Branch(table)) => table.for_each(f),
                // This is an orphan, we have nothing more to do.
				None => (),
            }
        }
    }

    /// Remove and run a closure on every key-value pair in the table.
    pub fn take_each<F: Fn(K, V)>(&self, f: F) {
        // Go over every bucket in the table.
        for i in self.buckets {
            // Remove the bucket from the table.
            match i.swap(None, atomic::Ordering::Acquire) {
				// There is a leaf; we simply apply the function to the inner value.
                Some(Node::Leaf(Pair { key, val })) => f(key, val),
				// There is a brnach; hence we must recurse with the call.
                Some(Node::Branch(table)) => table.take_each(f),
                // This is an orphan, we have nothing more to do.
				None => (),
            }
        }
    }
}
