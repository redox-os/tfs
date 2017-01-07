//! Ranges.
//!
//! This module contains the main primitive in zmicro.

/// A range.
///
/// This is the main primitive in the range encoding techniques. Ranges represent some variable
/// size block of the stream, which breaks down to bits.
///
/// Bits are read and written according to some probability. When a bit is written, the range
/// updates to a smaller subrange. The subrange's size is determined according to the probability
/// of the bit being 0. If the bit written is 0, and the probability pridiction was `Pr(0) = p`,
/// then the range's new size is `p·r` with `r` being the old size.
///
/// Ranges has an important invariant: They can never be of zero size. If a range is of size 1,
/// however, the range is said to be _exhausted_, meaning that it cannot store any more
/// information.
///
/// This concept might seem weird as first, but it is incredibly logical: The more correct the
/// prediction is, the less the range narrows, and consequently it is exhausted earlier, meaning
/// that it can store more information.
///
/// As such, the efficiency depends entirely on the predictor, and the only theoretical bound that
/// exists is the state space.
struct Range {
    /// The start of the range.
    start: u32,
    /// The length of the range.
    len: u32,
}

impl Range {
    /// Create a full size range.
    fn full() -> Range {
        Range {
            start: 0,
            len: !0,
        }
    }

    /// Write a bit into the range.
    ///
    /// This pushes `bit` to the stream represented in the range, with the probability `pr_0` of
    /// being 0 (`false`).
    ///
    /// `pr_0` linearly corresponds to the probability, but is represented as an integer for
    /// performance reasons. As such, 0.5 corresponds to 0x7FFFFFFF, 1 corresponds to 0xFFFFFFFF,
    /// and 0 corresponds to 0.
    ///
    /// The returned boolean is true if there is space for more bits.
    fn write(&mut self, bit: bool, pr_0: u32) -> bool {
        // Fetch the length for performance reasons.
        let len = self.len;

        debug_assert!(len > 1, "The current length of the range is too small to contain more \
                      bits, please renormalize/flush.");

        // Calculate the new length of the left subrange (i.e. the length of the range if the bit
        // is 0):
        //
        // - Cast the integers to 64-bit to avoid any overflows.
        // - Multiply the current length by the integer representing a probability. The integer
        //   corresponds linearly to the probability on the unit interval.
        // - Divide by the maximal value of the unnormalized probability, in order to normalize the
        //   result:
        //     - Add half the maximal value to shift the result and flip the results which would be
        //       have decimal parts above .5 to the next number.
        //     - Shift 32 bits downwards (equivalent to floored division by the maximal
        //       unnormalized probability).
        // - Cast back to 32-bit integer truncating only 0s because of the above bit shift.
        let mut len_0 = ((len as u64 * pr_0 as u64 + 0x7FFFFFFF) >> 32) as u32;

        // Normalize len_0 to avoid zero intervals.
        if len_0 == 0 {
            // len_0 rounded down to an empty range. This means that the left subrange for bit 0 is
            // empty, and thus not representable. For this reason, we round up to 1 just to make
            // the subrange representable.
            len_0 = 1;
        } else if len_0 == len {
            // len_0 rounded up to the length of the current range. This means that the right
            // subrange for bit 1 will be empty, and thus not representable. For this reason, we
            // leap to a range which is one unit shorter than the current one, leaving a right
            // subrange of length 1.
            len_0 = len - 1;
        }

        if bit {
            // The bit was 1.

            // Refine the range to the second half:
            //
            //     [            self.len           ]
            //     [  len_0  ][  self.len - len_0  ]
            //                \~~~~~~~~~~~~~~~~~~~~/
            //                 This is the new range
            self.len   -= len_0;
            self.start += len_0;

            // It's not exhausted if the length is stil above one.
            self.len > 1
        } else {
            // The bit was 0.

            // The start of the range is fixed, but update the length of the range.
            self.len = len_0;

            // We do the same as above, but for microoptimization we use len_0 instead as it could
            // have been register allocated.
            len_0 > 1
        }
    }

    /// Read a bit from the range.
    ///
    /// This reads the top bit (first written bit) with probability `pr_0` from the range, and
    /// updates the range such that the second bit is the new first (similar to `pop` but FIFO).
    ///
    /// The probability `pr_0` **must** match the probability given when the bit was written into
    /// the range.
    ///
    /// `None` is returned if the range is exhausted and no more bits are stored in the range.
    fn read(&mut self, pr_0: u32) -> Option<bool> {
        debug_assert!(self.len != 0, "Zero-sized ranges are invalid.");

        if self.len == !0 {
            // The range cannot contain more information, so no more bits can be extracted from
            // this range.
            None
        } else {
            debug_assert!(self.len != 0, "Zero-sized ranges are invalid.");

            // Construct the left half subrange of the full range according to the given
            // probability of 0 occuring.
            let mut left_half = Range::full();
            left_half.write(false, pr_0);

            // Determine if the start false on the left or right half.
            if self.start < left_half.len {
                // The bit is 0.

                // Rescale the length to get the left superrange:
                //
                //     [ full range           ]
                //         [ rescaled range ]
                //         [ left ][  right ]
                //
                // The left range is `left_half`, which is relative to the full range. Now, we
                // divide by the size of this range, but we double the width to get the desired
                // precision.
                self.len = (((self.len as u64) << 32) / left_half.len as u64) as u32;

                // The start of the range is fixed, because the read bit is zer.

                Some(false)
            } else {
                // The bit is 1.

                // Negate the range to get the right half of the range.
                let right_half = !0 - left_half.len;
                // We repeat the same as above conditional.
                self.len = (((self.len as u64) << 32) / right_half as u64) as u32;

                // Subtract the length of the left half to "shift" the range towards the left in
                // order to complete the transformation. This means that the new offset is the
                // start of the right range.
                self.start -= left_half.len;

                Some(true)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_read() {
        let mut range = Range::full();

        range.write(true, 5000000);
        range.write(true, 2999);
        range.write(false, 5000000000);
        range.write(false, 50000000);
        assert!(range.write(true, 333333332999));

        assert!( range.read(5000000).unwrap());
        assert!( range.read(2999).unwrap());
        assert!(!range.read(5000000000).unwrap());
        assert!(!range.read(50000000).unwrap());
    }

    #[test]
    fn write_ones() {
        let mut range = Range::full();

        let mut n = 0;
        while range.write(true, 500) {
            n += 1;
        }

        for i in 0..n {
            assert_eq!(range.read(500), Some(true));
        }

        assert_eq!(range.read(500), None);
    }
}
