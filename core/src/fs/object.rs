//! Unifying types and traits for on-disk structures.

use futures::Future;

use {fs, Error};

/// An on-disk object.
///
/// This trait encompasses types which represents on-disk objects. It defines certain operations
/// which such objects have in common.
pub trait Object {
    /// "Visit" the node as a part of the GC cycle.
    ///
    /// Garbage collection works by traversing a graph and creating a set of visited nodes. This
    /// visits the node (the object) and adds it to `visited`, and then visits its adjacent nodes.
    fn gc_visit(&self, fs: &fs::State) -> future!(());
}
