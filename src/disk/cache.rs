use std::sync::{self, Mutex, RwLock};
use std::ops;
use std::collections::HashMap;

use disk::{self, Disk};

const CACHE_LINE_SIZE: usize = 4096;
const CACHE_LINE_NUMBER: usize = 256;

struct Page {
    data: [u8; CACHE_LINE_SIZE],
    checksum: u64,
    dirty: bool,
}

pub struct Cached<D> {
    disk: D,
    lines: [Mutex<Option<Page>>; CACHE_LINES_NUMBER],
    map: RwLock<HashMap<u128, usize>>,
    cache: plru::Cache::MediumCache,
}

impl<D> Disk for Cached<D> where D: Disk {
    fn reset(&self) -> Result<()> {
        for i in &self.lines {
            *i = None;
        }
        self.map.write().unwrap().clear();
        self.dirty = sync::SegQueue::new();

        self.disk.reset()
    }

    fn read(&self, at: u128, buf: &mut [u8]) -> Result<()> {
        if Some(line) = self.map.read().get(at / CACHE_LINE_SIZE as u128) {
            let lock = self.lines[line].lock().unwrap();
            *lock.unwrap()
        }
    }

    fn write(&self, at: u128, buf: &[u8]) -> Result<()>;

    fn size(&self) -> u128 {
        self.disk.size()
    }

    fn flush(&self) -> Result<()>;
}
