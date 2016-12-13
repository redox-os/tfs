//! Cluster management.

use std::NonZero;

/// A pointer to some cluster.
pub struct Pointer(NonZero<u64>);

impl ClusterPointer {
    /// Create a new `ClusterPointer` to the `x`'th cluster.
    pub fn new(x: u64) -> Option<ClusterPointer> {
        if x == 0 {
            None
        } else {
            Some(ClusterPointer(unsafe {
                NonZero::new(x)
            }))
        }
    }
}

/// The cluster manager.
///
/// This is the center point of the I/O stack, providing allocation, deallocation, compression,
/// etc. It manages the clusters and caches the disks.
struct Manager<D> {
    /// The cached disk header.
    ///
    /// In reality, we could fetch this from the `disk` field as-we-go, but that hurts performance,
    /// so we cache it in memory.
    header: header::DiskHeader,
    /// The inner disk.
    disk: D,
}
