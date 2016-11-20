//! SeaHash: A blazingly fast, portable hash function with proven statistical guarantees.
//!
//! SeaHash is a hash function with performance similar (within around ±7% difference) to XXHash
//! and MetroHash, but with stronger guarantees.
//!
//! SeaHash is a portable hash function, meaning that the output is not dependent on the hosting
//! architecture, and makes no assumptions on endianness or the alike. This stable layout allows it
//! to be used for on-disk/permanent storage (e.g. checksums).
//!
//! SeaHash beats xxHash by 3-10% depending on the architecture in use. SeaHash has better quality
//! guarantees as well.
//!
//! # Achieving the performance
//!
//! Like any good general-purpose hash function, SeaHash reads 8 bytes at once effectively reducing
//! the running time by an order of ~5.
//!
//! Secondly, SeaHash achieves the performance by heavily exploiting Instruction-Level Parallelism.
//! In particular, it fetches 4 integers in every round and independently diffuses them. This
//! yields four different states, which are finally combined.
//!
//! # Statistical guarantees
//!
//! SeaHash comes with certain proven guarantees about the statistical properties of the output:
//!
//! 1. Pick some _n_-byte sequence, _s_. The number of _n_-byte sequence colliding with _s_ is
//!    independent of the choice of _s_ (all equivalence class have equal size).
//! 2. If you flip any bit in the input, the probability for any bit in the output to be flipped is
//!    0.5.
//!
//! The first guarantee can be derived through deduction, by proving that the diffusion function is
//! bijective (reverse the XORs and find the congruence inverses to the primes).
//!
//! The second guarantee requires more complex calculations: Construct a matrix of probabilities
//! and set one to certain (1), then apply transformations through the respective operations. The
//! proof is a bit long, but relatively simple.
//!
//! # Specification
//!
//! The hasher's state is 4-tuple of 4 64-bit integers, starting at `(0x16f11fe89b0d677c,
//! 0xb480a793d8e6c86c, 0x6fe2e5aaf078ebc9, 0x14f994a4c5259381)`. The input is split into blocks of
//! the size of the tuple. The block is split into 4 64-bit integers, each of which is added
//! (modulo 2⁶⁴) to the state, in matching order. The integers are read in little-endian.
//!
//! When the block is read and the state is updated, the diffusion is applied. The diffusion
//! function, `diffuse(x)`, is as follows:
//!
//! ```notest
//! x ← x ⊕ (x >> 32)
//! x ← px
//! x ← x ⊕ (x >> 32)
//! x ← px
//! x ← x ⊕ (x >> 32)
//! ```
//!
//! with `p = 0x7ed0e9fa0d94a33`.
//!
//! When the whole buffer (all blocks) is read, the state tuple, `(a, b, c, d)` is mixed into a
//! single number:
//!
//! ```notest
//! h = a + diffuse(b) + diffuse(c + diffuse(d))
//! ```
//!
//! In case that there is excessive bytes, each are added to the hash value, and then it's diffused
//! until all bytes are read.

#![no_std]
#![warn(missing_docs)]

use core::num::Wrapping as W;

/// The diffusion function cornerstone of SeaHash.
///
/// This is a bijective function emitting chaotic behavior. Such functions are used as building
/// blocks for hash functions.
#[inline(always)]
fn diffuse(mut x: W<u64>) -> W<u64> {
    x = x ^ (x >> 32);
    x = x * W(0x7ed0e9fa0d94a33);
    x = x ^ (x >> 32);
    x = x * W(0x7ed0e9fa0d94a33);
    x = x ^ (x >> 32);

    x
}

/// Hash some buffer.
pub fn hash(buf: &[u8]) -> u64 {
    // We use 4 different registers to store seperate hash states, because this allows us to update
    // them seperately, and consequently exploiting ILP to update the states in parallel.
    let mut a = W(0x16f11fe89b0d677c);
    let mut b = W(0xb480a793d8e6c86c);
    let mut c = W(0x6fe2e5aaf078ebc9);
    let mut d = W(0x14f994a4c5259381);

    // We fetch this in order to avoid aliasing things later on and thus breaking certain
    // optimizations.
    let len = buf.len() as isize;
    // We round down to a multiple of 32.
    let mut written_len = len & !0x1F;
    // We pre-fetch the pointer to the buffer to avoid too many cache misses.
    let buf_ptr = buf.as_ptr();
    // The pointer to the current bytes.
    let mut ptr = unsafe { buf_ptr.offset(written_len) } as *const u64;

    while (buf_ptr as usize) < ptr as usize {
        unsafe {
            // Read and diffuse the next 4 64-bit little-endian integers from their bytes. Note
            // that we on purpose not use `+=` and co., because it aliases the lvalue, making it
            // harder for LLVM to register allocate (it will have to inline the value behind the
            // pointer, effectively assuming that it is not aliased, which can be hard to prove).

            ptr = ptr.offset(-1);
            // Placing these updates inplace can have some negative consequences on especially
            // older architectures, where they can block ILP because they assume the evaluation of
            // the old `byte` is executed, which might trigger the diffusion to run serially.
            // However, not introducing a tmp register makes sure that you don't push from the
            // register to the stack, which comes with a performance hit.
            a = a + W((*ptr).to_le());

            ptr = ptr.offset(-1);
            b = b + W((*ptr).to_le());

            ptr = ptr.offset(-1);
            c = c + W((*ptr).to_le());

            ptr = ptr.offset(-1);
            d = d + W((*ptr).to_le());
        }

        // Diffuse the updated registers. We hope that each of these are executed in parallel.
        a = diffuse(a);
        b = diffuse(b);
        c = diffuse(c);
        d = diffuse(d);
    }

    // Diffuse `b` and `d` into `a` and `c`.
    a = a + diffuse(b);
    c = c + diffuse(d);

    // Diffuse `a` and `c`.
    let mut ret = a + diffuse(c);

    // Handle the excessive bytes.
    while written_len < len {
        // Increment the written bytes counter.
        written_len = written_len + 1;

        unsafe {
            // Update the return value by adding the excessive byte and then diffusing.
            ret = diffuse(ret + W(*buf_ptr.offset(written_len) as u64));
        }
    }

    ret.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero() {
        assert_eq!(hash(&[0; 4096]), 6601654217258774873);
    }

    #[test]
    fn seq() {
        let mut buf = [0; 4096];
        for i in 0..4096 {
            buf[i] = i as u8;
        }
        assert_eq!(hash(&buf), 10263305321322108915);
    }

    #[test]
    fn position_depedent() {
        let mut buf1 = [0; 4098];
        for i in 0..4098 {
            buf1[i] = i as u8;
        }
        let mut buf2 = [0; 4098];
        for i in 0..4098 {
            buf2[i] = i as u8 ^ 1;
        }

        assert!(hash(&buf1) != hash(&buf2));
    }

    #[test]
    fn shakespear() {
        assert_eq!(hash(b"to be or not to be"), 11897643864797330044);
    }

    #[test]
    fn not_equal() {
        assert_ne!(hash(b"to be or not to be "), hash(b"to be or not to be"));
        assert_ne!(hash(b"jkjke"), hash(b"jkjk"));
    }
}
