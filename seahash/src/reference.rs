//! A slow, but clear reference implementation of SeaHash.

use core::num::Wrapping as W;

use diffuse;

/// Read an integer in little-endian.
fn read_int(int: &[u8]) -> u64 {
    debug_assert!(int.len() <= 8, "The buffer length of the integer must be less than or equal to \
                  the one of an u64.");

    // Start at 0.
    let mut x = 0;
    for &i in int.iter().rev() {
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

        // Increment the cursor.
        self.cur += 1;
        // Wrap around.
        self.cur %= 4;
    }

    /// Calculate the final hash.
    fn finish(self, total: usize) -> W<u64> {
        // Even though addition is commutative, it doesn't matter, because the state vector's
        // initial components are mutually distinct, and thus swapping even and odd chunks will
        // affect the result, because it is sensitive to the initial condition. To add
        // discreteness, we diffuse.
        diffuse(self.vec[0]
            + self.vec[1]
            + self.vec[2]
            + self.vec[3]
            // We add in the number of written bytes to make it zero-sensitive when excessive bytes
            // are written (0u32.0u8 ≠ 0u16.0u8).
            + W(total as u64)
        )
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

    // Partition the rounded down buffer to chunks of 8 bytes, and iterate over them. The last
    // block might not be 8 bytes long.
    for int in buf.chunks(8) {
        // Read the chunk into an integer and write into the state.
        state.write_u64(read_int(int));
    }

    // Finish the hash state and return the final value.
    state.finish(buf.len()).0
}
