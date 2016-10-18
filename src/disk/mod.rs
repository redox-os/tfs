mod cache;

/// A physical or virtual medium which can be written and read from.
trait Disk: Default {
    /// Read from the disk starting from `at` to some particular buffer `buf`.
    ///
    /// It is assumed that we are the only reader/writer of the disk, and hence that reading a
    /// written segment will return the written data.
    fn read(&mut self, at: u128, buf: &mut [u8]);
    /// Write a buffer `buf` into the disk at `at`.
    fn write(&mut self, at: u128, buf: &[u8]);
    /// Atomically write a buffer `buf` into disk at `at`.
    fn atomic_write(&mut self, at: u128, buf: &[u8]);
    /// Get the size of this disk.
    fn size(&self) -> u128;
    /// Flush the cache, if any.
    fn flush(&mut self);
    /// Do a full disk data correction.
    ///
    /// This might be very expensive, due to needing traversal or long searching in order to find
    /// and correct the problems.
    fn fix(&mut self);
}
