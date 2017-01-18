//! Ranges.
//!
//! This module contains the main primitive in zmicro.

struct Range {
    start: u32,
    end: u32,
}

impl Range {
    fn write(&mut self, bit: bool, pr_0: u32) {
        let size_0 = (((self.end - self.start) as u64 << 32 + 0x80000000) / pr_0 as u64) as u32;

        if bit {
            self.start += size_0;
            self.end -= size_0;
        } else {
            self.end = self.start + size_0;
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
        range.write(false, 500000);
        range.write(false, 50000000);
        range.write(true, 333333);

        assert!( range.read(5000000).unwrap());
        assert!( range.read(2999).unwrap());
        assert!(!range.read(500000).unwrap());
        assert!(!range.read(50000000).unwrap());
        assert!( range.read(333333).unwrap());
    }

    #[test]
    fn write_ones() {
        let mut range = Range::full();

        let mut n = 0;
        while range.write(true, 500) {
            n += 1;
        }

        for _ in 0..n {
            assert_eq!(range.read(500), Some(true));
        }

        assert_eq!(range.read(500), None);
    }

    #[test]
    fn balanced_ones() {
        let mut range = Range::full();

        while range.write(true, 0x80000000) {}

        assert_eq!(range.start, 0xFFFFFFFF);
    }

    #[test]
    fn balanced_zeros() {
        let mut range = Range::full();

        while range.write(false, 0x80000000) {}

        assert_eq!(range.start, 0);
    }

    #[test]
    fn unbalanced_ones() {
        let mut range = Range::full();

        while range.write(true, 30482) {}

        assert_eq!(range.start, 0xFFFFFFFF);
    }
}
