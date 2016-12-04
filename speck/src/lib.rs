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

/// Encrypt a block with key schedule generated on-the-go.
///
/// This works great for one-time use of a key (such as usages other than encryption), because it
/// should never read from memory (both the message and the keys are stored in the registers). As
/// such, this should be really fast for such usage.
///
/// If you want to reuse the key, however, it is recommended that you use the precomputed schedule
/// provided by the `Key` struct.
pub fn encrypt_block((mut m1, mut m2): (u64, u64), (mut k1, mut k2): (u64, u64)) -> (u64, u64) {
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

/// A precomputed key.
///
/// This precomputes a key schedule, which can then be used for both encrypting and decrypting messages.
pub struct Key {
    /// The computed schedule.
    ///
    /// Each of these subkeys are used in a round of the cipher. The first subkey is used in the
    /// first round of the cipher and so on.
    schedule: [u64; ROUNDS as usize],
}

impl Key {
    /// Generate a new key from some seed.
    pub fn new((mut k1, mut k2): (u64, u64)) -> Key {
        let mut ret = Key {
            schedule: [0; ROUNDS as usize],
        };

        // Run `ROUNDS - 1` rounds to generate the key's endpoint (the last key in the schedule).
        for i in 0..ROUNDS {
            // Insert the key into the schedule.
            ret.schedule[i as usize] = k2;

            // The beautiful thing about Speck is that it reuses its round function to generate the
            // key schedule.
            round!(k1, k2, i);
        }

        ret
    }

    /// Encrypt a 128-bit block with this key.
    pub fn encrypt_block(&self, (mut m1, mut m2): (u64, u64)) -> (u64, u64) {
        // We run a round for every subkey in the generated key schedule.
        for &k in &self.schedule {
            // Run a round on the message.
            round!(m1, m2, k);
        }

        (m1, m2)
    }

    /// Decrypt a 128-bit block with this key.
    pub fn decrypt_block(&self, (mut c1, mut c2): (u64, u64)) -> (u64, u64) {
        // We run a round for every subkey in the generated key schedule.
        for &k in self.schedule.iter().rev() {
            // Run a round on the message.
            inv_round!(c1, c2, k);
        }

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

            let key = Key::new((x, y));

            assert_eq!(key.decrypt_block(key.encrypt_block((a, b))), (a, b));
            assert_eq!(key.encrypt_block((a, b)), encrypt_block((a, b), (x, y)));
        }
    }

    #[test]
    fn test_vectors() {
        // These test vectors are taken from the Speck paper.
        assert_eq!(encrypt_block((0x6c61766975716520, 0x7469206564616d20), (0x0f0e0d0c0b0a0908, 0x0706050403020100)), (0xa65d985179783265, 0x7860fedf5c570d18));
    }
}
