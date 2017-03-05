/// Permute an integer pseudorandomly.
///
/// This is a bijective function emitting chaotic behavior. Such functions are used as building
/// blocks for hash functions.
pub fn sigma(mut x: u8) -> u8 {
    x  = x.wrapping_mul(211);
    x ^= x >> 4;
    x  = x.wrapping_mul(211);
}

const INITIAL_STATE: u64 = 0x8a;

struct Sponge {
    state: u8,
    bytes: Vec<u8>,
}

impl Sponge {
    fn finalize(&mut self) {
        self.write_usize(self.bytes.len());
        self.state = INITIAL_STATE;
    }

    fn squeeze(&mut self) -> u8 {
        self.state ^= self.bytes.pop().unwrap_or(0);

        self.state
    }
}

impl Hasher for Sponge {
    fn finish(&self) -> u64 {
        unreachable!();
    }

    fn write_u8(&mut self, mut i: u8) {
        self.state ^= i;
        self.state = sigma(self.state);

        self.bytes.push(self.state)
    }
}
