//! Implementation of the Speck block cipher.
//!
//! Speck is a really simple block cipher designed by the NSA. It is famous for its simple
//! structure and code size, which can fit in just a couple of lines, while still preserving
//! security.

/// The number of rounds.
const ROUNDS: u64 = 32;

/// A single round of Speck.
///
/// This is a keyed ARX transformation.
macro_rules! round {
    ($x:ident, $y:ident, $k:ident) => {
        $x = $x.rotate_right(8);
        $x = $x.wrapping_add($y);
        $x ^= $k;
        $y = $y.rotate_left(3);
        $y ^= $x;
    }
}

/// Revert a Speck round given some subkey.
macro_rules! inv_round {
    ($x:ident, $y:ident, $k:ident) => {
        $y ^= $x;
        $y = $y.rotate_right(3);
        $x ^= $k;
        $x = $x.wrapping_sub($y);
        $x = $x.rotate_left(8);
    }
}

/// A key, which can be used to encrypt and decrypt messages.
pub struct Key {
    /// The left ending subkey of the schedule.
    end_k1: u64,
    /// The right ending subkey of the schedule.
    end_k2: u64,
    /// The left starting subkey of the schedule.
    start_k1: u64,
    /// The right starting subkey of the schedule.
    start_k2: u64,
}

impl Key {
    /// Generate a new key from some seed.
    pub fn new(mut k1: u64, mut k2: u64) -> Key {
        let mut ret = Key {
            start_k1: k1,
            start_k2: k2,
            end_k1: 0,
            end_k2: 0,
        };

        // Run `ROUNDS - 1` rounds to generate the key's endpoint (the last key in the schedule).
        for i in 0..ROUNDS - 1 {
            // The beautiful thing about Speck is that it reuses its round function to generate the
            // key schedule.
            round!(k1, k2, i);
        }

        // Set the generated keys as the endpoints.
        ret.end_k1 = k1;
        ret.end_k2 = k2;

        ret
    }

    /// Encrypt a 128-bit block with this key.
    pub fn encrypt_block(&self, (mut m1, mut m2): (u64, u64)) -> (u64, u64) {
        // Fetch the keys.
        let mut k1 = self.start_k1;
        let mut k2 = self.start_k2;

        // Run the initial round (similar to the loop below, but doesn't update the key schedule).
        round!(m1, m2, k2);

        for i in 0..ROUNDS - 1 {
            // Progress the key schedule.
            round!(k1, k2, i);
            // Run a round over the message.
            round!(m1, m2, k2);
        }

        (m1, m2)
    }

    /// Decrypt a 128-bit block with this key.
    pub fn decrypt_block(&self, (mut c1, mut c2): (u64, u64)) -> (u64, u64) {
        // Fetch the endpoint keys.
        let mut k1 = self.end_k1;
        let mut k2 = self.end_k2;

        for i in (0..ROUNDS - 1).rev() {
            // Run a round over the message.
            inv_round!(c1, c2, k2);
            // Revert the key update round.
            inv_round!(k1, k2, i);
        }

        debug_assert!(k1 == self.start_k1 && k2 == self.start_k2, "The start keys does not match \
                      the key derived from the end key.");

        // Revert the initial round.
        inv_round!(c1, c2, k2);

        (c1, c2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt() {
        let mut a = 394u64;
        let mut b = 320948u64;
        let mut x;
        let mut y = 234087328470234u64;

        for _ in 0..9000 {
            a = a.wrapping_mul(206066389);
            b ^= a;
            x = y.wrapping_add(a);
            y = x.wrapping_mul(b | 1);

            let key = Key::new(x, y);

            assert_eq!(key.decrypt_block(key.encrypt_block((a, b))), (a, b));
        }

    }

    #[test]
    fn test_vectors() {
        // These test vectors are taken from the Speck paper.
        let k = Key::new(0x0f0e0d0c0b0a0908, 0x0706050403020100);
        assert_eq!(k.encrypt_block((0x6c61766975716520, 0x7469206564616d20)), (0xa65d985179783265, 0x7860fedf5c570d18));
    }
}
