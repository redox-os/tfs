struct Pair<K, V> {
    key: K,
    val: V,
}

enum Node<K, V> {
    Leaf(Pair<K, V>),
    Branch(Table<K, V>),
}

impl Atomic<Node<K, V>> {
    /// Insert a key-value pair into the node without squeezing the sponge.
    fn insert(&self, pair: Pair<K, V>, sponge: Sponge) -> Option<V> {
        // If the entry was empty, we will set it to a leaf with our key-value pair. If not, we
        // will handle the respective cases manually.
        match self.cas(None, Some(Owned::new(Node::Leaf(pair))), ORDERING) {
            // We successfully set an empty entry to the new key-value pair. This of course implies
            // that the key didn't exist at the time.
            Ok(()) => None,
            // A node was found, so we call `insert` on it.
            Err(Some(node)) => node.insert(pair, sponge),
            // It is not possible to get `Err(None)` as that was the value we are CAS-ing against.
            Err(None) => unreachable!(),
        }
    }

    fn remove(&self, key: &K, sponge: Sponge) -> Option<V> {
        // Load the node and handle its cases.
        if let Some(node) = self.load(ORDERING) {
            // A node was found.
            node.remove(key, sponge);
        } else {
            // No node was found; nothing to remove.
            None
        }
    }

    fn for_each<F: Fn(K, V)>(&self, f: F) {
        if let Some(node) = i.load(ORDERING) {
            node.for_each(f);
        }
    }

    fn take_each<F: Fn(K, V)>(&self, f: F) {
        if let Some(node) = i.swap(Owned::new(None)) {
            node.take_each(f);
        }
    }
}

impl Shared<Node<K, V>> {
    fn insert(&self, pair: Pair<K, V>, sponge: Sponge) -> Option<V> {
        match self {
            // There is a branch table. Insert the key-value pair into it.
            Node::Branch(table) => table.insert(pair, sponge),
            // The key exists, so we can simply update the value.
            Node::Leaf(Pair { key }) if key == pair.key => Some(found_val.swap(val, ORDERING)),
            // Another key exists at the position, so we need to extend the table with a branch,
            // containing both entries.
            Node::Leaf(mut new_pair) => {
                // The reason we use recursion is that the entry could be deleted and another
                // re-inserted on its place. While this in theory acts like a spin-lock, I doubt it
                // will EVER run more than one time in the real world, as it would require the
                // entry to be removed in another thread at the same time, while a new one being
                // inserted (requiring a whole lookup!) in the meantime on the same very place. It
                // is pretty damn unlikely, but it is needed for correctness nonetheless.

                // Create a table that contains both the key-value pair we're inserting and the one
                // on the place, where we want to insert.
                let new_table = Table::two_entries(pair, new_pair);
                // We try to update the current entry to our table. The reason we use CAS is that
                // we want to ensure that the entry was not changed in the meantime, so we compare
                // to `new_pair`, which must be a leaf with the old key-value pair, as the epoch
                // system ensures that it doesn't change while we have the reference (therefore
                // there is no ABA problem here). So in essence, we check that our value is still
                // the same as the original, and if it is we update it. If not, we must handle the
                // new value, which could be anything else (e.g. another thread could have extended
                // the leaf too because it is inserting the same pair).
                match entry.cas(new_pair, Some(Owned::new(Node::Branch(new_table))), ORDERING) {
                    // Our update went smooth, and we have extended the leaf to a branch,
                    // meaning that there now is a subtable containing the two key-value pairs.
                    Ok(()) => None,
                    // Something else was inserted in the meantime, so we must re-do the
                    // insertions.
                    Err(Some(node)) => node.insert(pair, sponge),
                    // The entry was removed, so we will again recurse to re-do.
                    Err(None) => entry.insert(pair, sponge),
                }
            },
        }
    }

    fn remove(&self, key: &K, sponge: Sponge) -> Option<V> {
        match self {
            // There is a branch, so we must remove the key in the sub-table.
            Node::Branch(table) => table.remove(key, sponge),
            // There was a node with the key, which we will try to remove. We use CAS in order to
            // make sure that it is the same node as the one we read (`self`), otherwise we might
            // remove a wrong node.
            Node::Leaf(Pair { found_key, val }) if found_key == key
                => match entry.cas(Some(self), None, ORDERING) {
                // Removing the node succeeded: It wasn't changed in the meantime.
                Ok(()) => Some(val),
                // The node was removed by another thread, so our job is done. We don't need to do
                // anything.
                Err(None) => None,
                // To solve the ABA problem, we're unfortunately forced to recurse/loop. As in
                // `insert()`, this is theoretically a spin-lock, however it is very rare to run
                // more than once, as it requires another thread to have removed the leaf and then
                // inserted a new one in the short meantime spanning only a few CPU cycles.
                Err(Some(node)) => node.remove(),
            },
            // A node with a non-matching key was found. Hence, we have nothing to remove.
            Node::Leaf(..) => None,
        }
    }

    fn for_each<F: Fn(K, V)>(&self, f: F) {
        match self {
            Node::Leaf(Pair { key, val }) => f(key, val),
            Node::Branch(table) => table.for_each(f),
        }
    }

    fn take_each<F: Fn(K, V)>(&self, f: F) {
        match self {
            Node::Leaf(Pair { key, val }) => f(key, val),
            Node::Branch(table) => table.take_each(f),
        }
    }
}

struct Table<K, V>  {
    table: [Atomic<Node<K, V>>; 256],
}

impl Table<K, V> {
    fn get(&self, key: &K, sponge: Sponge) -> Option<Shared<V>> {
        // Load the entry and handle the respective cases.
        match self.table[sponge.squeeze()].load(ORDERING) {
            // The entry was a leaf and the keys match, so we can return the entry's value.
            Some(Node::Leaf(Pair { found_key, found_val })) if key == found_key => Some(found_val),
            // The entry is a branch with another table, so we recurse and look up in said
            // subtable.
            Some(Node::Branch(table)) => table.get(pair, sponge),
            // The entry is either a leaf but doesn't match, or is a null pointer, meaning there is
            // no entry with the key.
            Some(Node::Leaf(_)) | None => None,
        }
    }

    fn remove(&self, key: &K, sponge: Sponge) -> Option<V> {
        // We squeeze the sponge to get the right entry of our table, in which we will potentially
        // remove the key.
        self.table[sponge.squeeze()].remove(key, sponge);
    }

    fn insert(&self, pair: Pair<K, V>, sponge: Sponge) -> Option<V> {
        // We squeeze the sponge to get the right entry of our table, in which we will insert our
        // key-value pair.
        self.table[sponge.squeeze()].load(ORDERING).insert(pair, sponge)
    }

    fn take_each<F: Fn(K, V)>(&self, f: F) {
        for i in self.table {
            i.take_each(f);
        }
    }
}
