//! Cluster management.

use std::NonZero;

/// The size (in bytes) of the cluster header.
const HEADER: usize = 4;
/// The size (in bytes) of a cluster (without header).
const SIZE: usize = disk::SECTOR_SIZE - HEADER;
/// The size (in bytes) of a cluster pointer.
const POINTER_SIZE: usize = 8;

/// A pointer to some cluster.
pub struct Pointer(NonZero<u64>);

impl ClusterPointer {
    /// Create a new `ClusterPointer` to the `x`'th cluster.
    ///
    /// This returns `None` if `x` is `0`.
    pub fn new(x: u64) -> Option<ClusterPointer> {
        if x == 0 {
            None
        } else {
            // This is safe due to the above conditional.
            Some(ClusterPointer(unsafe {
                NonZero::new(x)
            }))
        }
    }
}
