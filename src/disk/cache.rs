use std::sync::RwLock;
use std::collections::HashMap;

use disk::Disk;

struct Page {
    data: [u8; 4096],
    checksum: u64,
}

pub struct Cached<D> {
    disk: D,
    /// The dirty cache lines pending on a flush.
    ///
    /// The ordering is highly important, and they should never be executed the wrong order, since
    /// this can lead to an inconsistent state.
    dirty: sync::SegQueue<usize>,
    lines: [Option<Mutex<[u8; CACHE_LINE_SIZE]>>; CACHE_LINES_NUMBER],
    map: RwLock<HashMap<u128, usize>>,
}

impl<D> Disk for Cached<D> where D: Disk {
    fn read(&mut self, at: u128, buf: &mut [u8]) {
        if self.cache.read(at, buf).is_err() {
            self.disk.read(at, buf);
        }
    }

    fn write(&mut self, at: u128, buf: &[u8]) {
        self.cache.write(at, buf);
    }

    fn atomic_write(&mut self, at: u128, buf: &[u8]) {
    }

    fn size(&self) -> u128 {
        self.disk.size()
    }

    fn flush(&mut self) {
        self.cache.flush(&mut self.disk);
    }

    fn fix(&mut self) {
        self.disk.fix();
    }
}
