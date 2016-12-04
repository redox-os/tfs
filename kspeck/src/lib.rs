//! KSpeck: Bijective key derivation function based on the Speck cipher.
//!
//! KSpeck is a tunable bijective KDF based on the Speck block cipher. It's based on the "scrypt"
//! paper, but tweaked to use a block cipher instead of a hash function.
//!
//! It is collision-free because it is a bijective function. As such, the worst-case is that an
//! attack is able to quickly generate the key. The attack can never reduce the key space, and
//! random bruteforcing is provably as hard as without KSpeck. However, KSpeck makes it harder to
//! perform dictionary attacks by consuming a custom amount of memory to generate the key schedule.
//!
//! In short, the algorithm consists of memory-intensive PRF, which maps the seed and round to some
//! metakey. This metakey is then used to encrypt the key.
//!
//! The hardness depends on the security of the underlying cipher (Speck cipher).

extern crate speck;

/// Derive the metakey.
///
/// This function takes a pre-specified amount of memory and number of rounds.
fn metakey(seed: (u64, u64), mem: u64, rounds: u64) -> (u64, u64) {
    let mut pad = Vec::with_capacity(mem as usize);

    // The initial value of the metakey is simply zero.
    let mut ret = (0, 0);

    // Generate the keypad.
    for n in 0..mem {
        // Encrypt the metakey with the seed. It is very important that we do not use e.g. CTR-mode
        // or other random-access CSPRNG constructions, because they allow us to fetch the values
        // on-the-go, meaning that the memory storage can be circumvented. By depending on the
        // previous key, you ensure that all the keys have effect on later keys.
        ret = speck::encrypt_block(ret, seed);
        // Push the metakey (it is needed later for random-access key generation).
        pad.push(ret);
    }

    // We won't reset `ret`, because it depends on all values in the keypad, making it
    // impossible to extend the keypad on-the-go.

    // Pseudo-randomly walk around the keypad and encrypt the metakey with values from the keypad.
    for n in 0..rounds {
        // Encrypt the metakey with the keypad entry given by the metakey itself modulo the length
        // of the keypad.
        ret = speck::encrypt_block(ret, pad[(ret.0 % mem) as usize]);
    }

    ret
}

/// Transform a key by a seed with computational parameters.
///
/// This does computationally intensive calculations, which one-to-one transforms a key.
///
/// The basic algorithm is as follows: Generate a pseudorandom stream with each entry being defined
/// by some function of the previous. This stream is traversed by some pseudorandom walk
/// (recursively defined), whose visited nodes are used to encrypt some "metakey", which will
/// finally be used to encrypt the original key.
pub fn generate(mut key: (u64, u64), seed: (u64, u64), mem: u64, rounds: u64) -> (u64, u64) {
    // Generate the metakey through a space-and-time-hard algorithm, then use it as key to encrypt
    // the input key.
    speck::encrypt_block(key, metakey(seed, mem, rounds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big() {
        assert_eq!(generate((2398, 2383), (8207408370234, 274387329874892734), 524288, 524288),
                   (9720246691184558256, 11056056011571954427));
    }
}
