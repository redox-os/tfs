//! Implementation of the SPECK block cipher.
//!
//! SPECK is a really simple block cipher designed by the NSA. It is famous for its simple
//! structure and code size, which can fit in just a couple of lines, while still preserving
//! security.
#![no_std]
#![forbid(unsafe_code)]

use core::fmt;

/// The number of rounds.
const ROUNDS: u64 = 32;

/// A single round of SPECK.
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

/// Revert a SPECK round given some subkey.
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
pub fn encrypt_block(m: u128, k: u128) -> u128 {
    let mut m1 = (m >> 64) as u64;
    let mut m2 = m as u64;
    let mut k1 = (k >> 64) as u64;
    let mut k2 = k as u64;

    // Run the initial round (similar to the loop below, but doesn't update the key schedule).
    round!(m1, m2, k2);

    for i in 0..ROUNDS - 1 {
        // Progress the key schedule.
        round!(k1, k2, i);
        // Run a round over the message.
        round!(m1, m2, k2);
    }

    m2 as u128 | (m1 as u128) << 64
}

/// A precomputed key.
///
/// This precomputes a key schedule, which can then be used for both encrypting and decrypting
/// messages.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    /// The computed schedule.
    ///
    /// Each of these subkeys are used in a round of the cipher. The first subkey is used in the
    /// first round of the cipher and so on.
    schedule: [u64; ROUNDS as usize],
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl Key {
    /// Generate a new key from some seed.
    pub fn new(k: u128) -> Key {
        let mut k1 = (k >> 64) as u64;
        let mut k2 = k as u64;

        let mut ret = Key {
            schedule: [0; ROUNDS as usize],
        };

        // Run `ROUNDS - 1` rounds to generate the key's endpoint (the last key in the schedule).
        for i in 0..ROUNDS {
            // Insert the key into the schedule.
            ret.schedule[i as usize] = k2;

            // The beautiful thing about SPECK is that it reuses its round function to generate the
            // key schedule.
            round!(k1, k2, i);
        }

        ret
    }

    /// Encrypt a 128-bit block with this key.
    pub fn encrypt_block(&self, m: u128) -> u128 {
        let mut m1 = (m >> 64) as u64;
        let mut m2 = m as u64;

        // We run a round for every subkey in the generated key schedule.
        for &k in &self.schedule {
            // Run a round on the message.
            round!(m1, m2, k);
        }

        m2 as u128 | (m1 as u128) << 64
    }

    /// Decrypt a 128-bit block with this key.
    pub fn decrypt_block(&self, c: u128) -> u128 {
        let mut c1 = (c >> 64) as u64;
        let mut c2 = c as u64;

        // We run a round for every subkey in the generated key schedule.
        for &k in self.schedule.iter().rev() {
            // Run a round on the message.
            inv_round!(c1, c2, k);
        }

        c2 as u128 | (c1 as u128) << 64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt() {
        for mut x in 0u128..90000 {
            // <3
            x = x.wrapping_mul(0x6eed0e9da4d94a4f6eed0e9da4d94a4f);
            x ^= (x >> 6) >> (x >> 122);
            x = x.wrapping_mul(0x6eed0e9da4d94a4f6eed0e9da4d94a4f);

            let key = Key::new(!x);

            assert_eq!(key.decrypt_block(key.encrypt_block(x)), x);
            assert_eq!(key.encrypt_block(x), encrypt_block(x, !x));
        }
    }

    #[test]
    fn test_vectors() {
        // These test vectors are taken from the SPECK paper.
        assert_eq!(
            encrypt_block(
                0x6c617669757165207469206564616d20,
                0x0f0e0d0c0b0a09080706050403020100
            ),
            0xa65d9851797832657860fedf5c570d18
        );
    }
}
