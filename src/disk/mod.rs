mod cache;

type Result<T> = Result<T, Error>;

pub enum Error {
    UnableToRead,
    UnableToWrite,
    Inconsistency,
    Unrecoverable,
}

/// A physical or virtual medium which can be written and read from.
pub trait Disk {
    /// Reset the medium to a valid, initial state.
    fn reset(&mut self) -> Result<()>;

    /// Read from the disk starting from `at` to some particular buffer `buf`.
    ///
    /// It is assumed that we are the only reader/writer of the disk, and hence that reading a
    /// written segment will return the written data.
    fn read(&mut self, at: u128, buf: &mut [u8]) -> Result<()>;
    /// Write a buffer `buf` into the disk at `at`.
    fn write(&mut self, at: u128, buf: &[u8]) -> Result<()>;
    /// Atomically write a buffer `buf` into disk at `at`.
    fn atomic_write(&mut self, at: u128, buf: &[u8]) -> Result<()>;
    /// Get the size of this disk.
    fn size(&self) -> u128;
    /// Flush the cache, if any.
    fn flush(&mut self) -> Result<()>;
}
