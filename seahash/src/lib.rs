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
//! # Advantages over other hash functions
//!
//! - Portability: SeaHash always gives the same hash across any platforms, and can thus be used
//!   for e.g. on-disk structures.
//! - Performance: SeaHash beats every high-quality (grading 10/10 in smhasher) hash function that
//!   I know off.
//! - Hardware accelerateable: SeaHash is designed such that ASICs can implement it with really
//!   high performance.
//! - Provable quality guarantees: Contrary to most other non-cryptographic hash function, SeaHash
//!   can be proved to satisfy the avalanche criterion as well as BIC.
//!
//! # Statistical guarantees
//!
//! SeaHash comes with certain proven guarantees about the statistical properties of the output:
//!
//! 1. Pick some _n_-byte sequence, _s_. The number of _n_-byte sequence colliding with _s_ is
//!    independent of the choice of _s_ (all equivalence class have equal size).
//! 2. If you flip any bit in the input, the probability for any bit in the output to be flipped is
//!    0.5.
//! 3. The hash value of a sequence of uniformly distributed bytes is itself uniformly distributed.
//!
//! The first guarantee can be derived through deduction, by proving that the diffusion function is
//! bijective (reverse the XORs and find the congruence inverses to the primes).
//!
//! The second guarantee requires more complex calculations: Construct a matrix of probabilities
//! and set one to certain (1), then apply transformations through the respective operations. The
//! proof is a bit long, but relatively simple.
//!
//! The third guarantee requires proving that the hash value is a tree, such that:
//! - Leafs represents the input values.
//! - Single-child nodes reduce to the diffusion of the child.
//! - Multiple-child nodes reduce to the sum of the children.
//!
//! Then simply show that each of these reductions transform uniformly distributed variables to
//! uniformly distributed variables.
//!
//! # Inner workings
//!
//! In technical terms, SeaHash follows a alternating 4-state length-padded Merkle–Damgård
//! construction with an add-diffuse compression function:
//!
//! ![A diagram.](http://ticki.github.io/img/seahash_construction_diagram.svg)
//!
//! It starts with 4 initial states, then it alternates between them (increment, wrap on 4) and
//! does modular addition with the respective block. When a state has been visited the diffusion
//! function (f) is applied. The very last block is padded with zeros.
//!
//! After all the blocks have been gone over, all the states are added to the number of bytes
//! written. The sum is then passed through the diffusion function, which produces the final hash
//! value.
//!
//! The diffusion function is drawn below.
//!
//! ```notest
//! x ← x ≫ 32
//! x ← px
//! x ← x ≫ 32
//! x ← px
//! x ← x ≫ 32
//! ```
//!
//! The advantage of having four completely segregated (note that there is no mix round, so they're
//! entirely independent) states is that fast parallelism is possible. For example, if I were to
//! hash 1 TB, I can spawn up four threads which can run independently without _any_
//! intercommunication or syncronization before the last round.
//!
//! If the diffusion function (f) was cryptographically secure, it would pass cryptoanalysis
//! trivially. This might seem irrelavant, as it clearly isn't cryptographically secure, but it
//! tells us something about the inner semantics. In particular, any diffusion function with
//! sufficient statistical quality will make up a good hash function in this construction.

#![no_std]
#![warn(missing_docs)]

use core::num::Wrapping as W;

pub use buffer::hash;
pub use stream::SeaHasher;

pub mod reference;
mod buffer;
mod stream;

/// The diffusion function.
///
/// This is a bijective function emitting chaotic behavior. Such functions are used as building
/// blocks for hash functions.
fn diffuse(mut x: W<u64>) -> W<u64> {
    // Move entropy down by XOR with shifting.
    x ^= x >> 32;
    // Move entropy up by scattering through multiplication.
    x *= W(0x7ed0e9fa0d94a33);
    // We still need more entropy downwards. Flipping higher bits won't flip lower ones, so far.
    // For example, if you flip the most significant bit, the 32'th bit will flip per the XOR-shift
    // subdiffusion, but this flip will only be scattered by the multiplication to flipping bits
    // higher than the 32'th, meaning that the ones lower will be unaffected. As such, we need to
    // get some entropy down.
    x ^= x >> 32;
    // So far, the avalanche diagram looks pretty good, but it still emits stripe patterns. For
    // example, flipping the 5'th lowest bit won't flip the least significant bit because of the
    // choice of scalar (in particular, observe how it leaves the 32'th bit unflipped after the
    // multiplication, which means that the XOR-shift never affects the lowest bit). No choice of
    // scalar will make this go away, it will merely change the unaffected bits. Instead, we need
    // to make the behavior more undeterministic by scattering bits through multiplication.
    x *= W(0x7ed0e9fa0d94a33);
    // This is the final stage of the diffusion function. There are still issues with the lowest
    // bits, which are still unaffected by the multiplication above. However, the multiplication
    // solved the higher bits' dependence, so lending entropy from the higher half will fix the
    // issues with the lower half.
    x ^= x >> 32;

    x
}
