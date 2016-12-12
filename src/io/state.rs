//! I/O state.

/// The I/O state.
///
/// This is the center point of the I/O stack, providing allocation, deallocation, compression,
/// etc. It manages the clusters and caches the disks.
struct State<D> {
    /// The cached disk header.
    ///
    /// In reality, we could fetch this from the `disk` field as-we-go, but that hurts performance,
    /// so we cache it in memory.
    header: header::DiskHeader,
    /// The inner disk.
    disk: D,
}
