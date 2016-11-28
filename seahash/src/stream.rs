use core::hash::Hasher;

use {hash, diffuse};

/// The streaming version of the algorithm.
///
/// Note that the input type is not taken into account, and thus is assumed to be fixed.
pub struct SeaHasher {
    /// The state of the hasher.
    state: u64,
}

impl Default for SeaHasher {
    fn default() -> SeaHasher {
        SeaHasher {
            state: 0xba663d61fe3aa408,
        }
    }
}

impl SeaHasher {
    /// Create a new `SeaHasher` with default state.
    pub fn new() -> SeaHasher {
        SeaHasher::default()
    }
}

impl Hasher for SeaHasher {
    fn finish(&self) -> u64 {
        self.state
    }

    fn write(&mut self, bytes: &[u8]) {
        self.state ^= hash(bytes);
        self.state = diffuse(self.state);
    }

    fn write_u64(&mut self, n: u64) {
        self.state ^= n;
        self.state = diffuse(self.state);
    }

    fn write_u8(&mut self, n: u8) {
        self.write_u64(n as u64);
    }

    fn write_u16(&mut self, n: u16) {
        self.write_u64(n as u64);
    }

    fn write_u32(&mut self, n: u32) {
        self.write_u64(n as u64);
    }

    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
}
