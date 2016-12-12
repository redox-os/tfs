//! Clusters.

use std::NonZero;

/// A pointer to some cluster.
pub struct ClusterPointer(NonZero<u64>);

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
