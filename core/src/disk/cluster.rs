//! Clusters.

use little_endian;

/// The size (in bytes) of a cluster pointer.
pub const POINTER_SIZE: usize = 8;

/// A pointer to some cluster.
// TODO: Use `NonZero`.
pub struct Pointer(u64);

impl little_endian::Encode for Pointer {
    fn write_le(self, into: &mut [u8]) {
        if let Some(ptr) = self {
            // Simply write the inner pointer into the buffer.
            little_endian::write(into, self)
        } else {
            // Zero the first `POINTER_SIZE` bytes of the buffer (null pointer).
            for i in &mut into[..POINTER_SIZE] {
                *i = 0;
            }
        }
    }
}

impl little_endian::Decode for Option<Pointer> {
    fn read_le(from: &[u8]) -> Option<Pointer> {
        if &from[..POINTER_SIZE] == &[0; POINTER_SIZE] {
            // The pointer was null, so we return `None`.
            None
        } else {
            // The pointer wasn't null, so we can simply read it as an integer. Note that we have
            // already ensured that it is not null, so it is safe.
            Some(Pointer(little_endian::read(from)))
        }
    }
}

impl little_endian::Encode for Option<Pointer> {
    fn write_le(self, into: &mut [u8]) {
        if let Some(ptr) = self {
            // Simply write the inner pointer into the buffer.
            little_endian::write(into, self)
        } else {
            // Zero the first `POINTER_SIZE` bytes of the buffer (null pointer).
            for i in &mut into[..POINTER_SIZE] {
                *i = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn null_pointer() {
        assert!(little_endian::read(&[0; POINTER_SIZE]).is_none());
    }

    fn non_null_pointer() {
        let original_buf = &[2, 0, 0, 0, 0, 0, 0, 0];
        let ptr = little_endian::read(original_buf).unwrap();
        let mut buf = [0; 8];
        little_endian::write(&mut buf, ptr);

        assert_eq!(original_buf, buf);
    }
}
