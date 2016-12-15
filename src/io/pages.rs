//! Page management.
//!
//! Pages are virtual data units of size 4088 bytes. They're represented on disk somewhat
//! non-obviously, since clusters can hold more than one page at once (compression). Every cluster
//! will maximize the number of pages held and when it's filled up, a new cluster will be fetched.

/// The page manager.
///
/// This is the center point of the I/O stack, providing allocation, deallocation, compression,
/// etc. It manages the clusters (with the page abstraction) and caches the disks.
struct Manager<D> {
    /// The cached disk header.
    ///
    /// In reality, we could fetch this from the `disk` field as-we-go, but that hurts performance,
    /// so we cache it in memory.
    header: header::DiskHeader,
    /// The inner disk.
    disk: D,
}
