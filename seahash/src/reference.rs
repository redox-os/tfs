//! A slow, but clear reference implementation of SeaHash.

use core::num::Wrapping as W;

/// The diffusion function.
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

/// Read an integer in little-endian.
fn read_int(int: &[u8]) -> u64 {
    // Start at 0.
    let mut x = 0;
    for &i in int {
        // Shift up a byte.
        x <<= 8;
        // Set the lower byte.
        x |= i as u64;
    }

    x
}

/// A hash state.
struct State {
    /// The state vector.
    vec: [W<u64>; 4],
    /// The component of the state vector which is currently being modified.
    cur: usize,
    /// The number of bytes written into the state.
    written: usize,
}

impl State {
    /// Write a 64-bit integer to the state.
    fn write_u64(&mut self, x: u64) {
        // Mix it into the substate by adding it.
        self.vec[self.cur] += W(x);
        // Diffuse the component to remove deterministic behavior and commutativity.
        self.vec[self.cur] = diffuse(self.vec[self.cur]);

        if self.cur == 0 {
            // The component pointer was zero; wrap.
            self.cur = 3;
        } else {
            // Simply decrement.
            self.cur -= 1;
        }

        self.written += 8;
    }

    /// Write 7 or less excessive bytes to
    fn write_excessive(&mut self, mut buf: &[u8]) {
        // Ensure that the invariants are true.
        debug_assert!(buf.len() < 8, "The buffer length of the excessive bytes must be less than an\
                      u64.");

        // Update the number of written bytes.
        self.written += buf.len();

        // We go to the first component for rather complicated reasons. The short version is that
        // doing this allows us to decrease code size in the optimized version.
        self.cur = 0;

        // Write the excessive u32 (if any).
        if buf.len() >= 4 {
            // Read the u32 into a u64, and write it into the state.
            self.write_u64(read_int(&buf[4..]));
            // Shift the buffer.
            buf = &buf[4..];
        }

        // Write the excessive u16 (if any).
        if buf.len() >= 2 {
            // Read the u16 into a u64, and write it into the state.
            self.write_u64(read_int(&buf[2..]));
            // Shift the buffer.
            buf = &buf[2..];
        }

        // Write the excessive u8 (if any).
        if buf.len() >= 1 {
            // Write the remaining byte into the state.
            self.write_u64(buf[0] as u64);
        }
    }

    /// Calculate the final hash.
    fn finish(self) -> W<u64> {
        // This is calculated like a Merkle tree, but because concatenation makes no sense in the
        // context and we use addition instead, we need to get rid of the commutativity, which we
        // do by diffusing the right child of every node and finally adding it all together.
        self.vec[0]
            // We add in the number of written bytes to make it zero-sensitive when excessive bytes
            // are written (0u32.0u8 ≠ 0u16.0u8).
            + diffuse(self.vec[1] + W(self.written as u64))
            + diffuse(self.vec[2] + diffuse(self.vec[3]))
    }
}

impl Default for State {
    fn default() -> State {
        State {
            // These values are randomly generated, and can be changed to anything (you could make
            // the hash function keyed by replacing these.)
            vec: [
                W(0x16f11fe89b0d677c),
                W(0xb480a793d8e6c86c),
                W(0x6fe2e5aaf078ebc9),
                W(0x14f994a4c5259381),
            ],
            // We start at the first component.
            cur: 0,
            // Initially, no bytes are written.
            written: 0,
        }
    }
}

/// A reference implementation of SeaHash.
///
/// This is bloody slow when compared to the optimized version. This is because SeaHash was
/// specifically designed to take all sorts of hardware and software hacks into account to achieve
/// maximal performance, but this makes code significantly less readable. As such, this version has
/// only one goal: to make the algorithm readable and understandable.
pub fn hash(buf: &[u8]) -> u64 {
    // Initialize the state.
    let mut state = State::default();

    // Round down the buffer length to the nearest multiple of 8 (aligned to u64).
    let rounded_down_len = buf.len() / 8 * 8;

    // Partition the rounded down buffer to chunks of 8 bytes, and iterate over them in reversed
    // order.
    for int in buf[..rounded_down_len].windows(8).rev() {
        // Read the chunk into an integer and write into the state.
        state.write_u64(read_int(int));
    }

    // Write the excessive bytes.
    state.write_excessive(&buf[rounded_down_len..]);

    // Finish the hash state and return the final value.
    state.finish().0
}
