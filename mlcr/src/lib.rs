//! MLCR: Machine-Learning-based Cache Replacement
//!
//! MLCR trains a neural network to "guess" how long time will pass before the cache block is
//! accessed again. In other words, it provides a qualified guess to approximate the ideal Bélády's
//! algorithm without a time machine.
//!
//! MLCR is slow, because it needs to train a neural network, but in many cases, the added
//! precision pays off by greatly reducing the number of cache misses. As such, it should only be
//! used when the cached medium is significantly slower than training the network (e.g. hard disks or
//! internet downloads).

extern crate crossbeam;
extern crate nn;
extern crate parking_lot;

use crossbeam::sync::SegQueue;
use nn::NN;
use parking_lot::{Mutex, MutexGuard};

use std::{cmp, f64};
use std::collections::{BinaryHeap, HashMap};

/// A clock tick count.
///
/// Every touch (i.e. read) increments the _clock_ yielding a new _tick_. This tick is roughly used
/// as a measure for the time passed (the actual time is irrelevant as it doesn't change the state
/// of the cache).
///
/// This tick count is used in the neural network model for the next hit prediction.
type Tick = u32;
/// The ID of a cache block.
///
/// The ID uniquely identifies a particular cache block inhabitant. It is used in the prediction
/// model and should thus be chosen carefully as representing the inner data (e.g. the disk
/// address) in order to achieve least cache misses.
pub type Id = u64;

/// A cache block.
///
/// This represents the state of a particular cache block.
struct Block {
    /// The two last times the block was used.
    last_used: [Tick; 2],
    /// The tick where the block was added.
    instated: Tick,
    /// The number of times the block has been touched.
    times_used: u32,
}

impl Block {
    /// Convert the block data into a vector.
    fn as_vec(&self, id: Id) -> Vec<f64> {
        vec![id as f64, self.instated as f64, self.last_used[0] as f64, self.last_used[1] as f64,
             self.times_used as f64]
    }
}

/// A next usage prediction.
///
/// This contains a prediction produced by the neural network, estimating when is the next tick,
/// the block will be touched.
#[derive(PartialEq)]
struct Prediction {
    /// The ID of the block we're predicting.
    id: Id,
    /// The prediction produced by the neural network.
    ///
    /// Note that this does not represent a tick, but rather a monotone function thereof.
    prediction: f64,
}

impl cmp::Ord for Prediction {
    fn cmp(&self, other: &Prediction) -> cmp::Ordering {
        if self.prediction < other.prediction {
            cmp::Ordering::Less
        } else {
            cmp::Ordering::Greater
        }
    }
}

impl cmp::PartialOrd for Prediction {
    fn partial_cmp(&self, other: &Prediction) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl cmp::Eq for Prediction {}

/// An iterator over the coldest (best candidates for replacement) to hotter cache objects.
///
/// This iterators from the objects predicted to be used in the farthest future to the nearest
/// future.
///
/// In other words, this goes over the best to worse candidates for replacement, trimming, or
/// clearing.
pub struct ColdIter {
    /// A binary heap over the predictions ordered by distance into the future.
    heap: BinaryHeap<Prediction>,
}

impl Iterator for ColdIter {
    type Item = Id;

    fn next(&mut self) -> Option<Id> {
        self.heap.pop().map(|Prediction { id, .. }| id)
    }
}

/// A learning cache tracker.
///
/// This keeps track of cache blocks.
///
/// A cache block represents some data, which is not managed by the cache tracker. The cache block
/// is said to be _touched_ when this data is used in some way.
///
/// The _ideal replacement_ is the block which is used in the most distant future. As this is not
/// possible to know in advance, we make a prediction or a _approximate ideal replacement_, which
/// is based around various data points of the block such as the time of the last uses, or the
/// number of touches.
///
/// The aim of the cache tracker is to provided _approximate ideal replacements_. Numerous
/// algorithms for making these predictions exists (examples are LRU, PLRU, LFU, MFU, MRU, ARC,
/// etc.), but MLCR uses an approach which is radically different: It feeds the data points into a
/// neural network and lets this estimate the tick of the next touch.
pub struct Cache {
    /// The blocks in this cache tracker.
    blocks: HashMap<Id, Block>,
    /// The neural network mapping blocks to the ticks of next touch.
    nn: NN,
    /// The clock.
    ///
    /// This increments on every touch.
    clock: Tick,
}

impl Cache {
    /// Tick the clock.
    fn tick(&mut self) {
        self.clock += 1;
    }

    /// Create a new cache tracker.
    pub fn new() -> Cache {
        Cache {
            blocks: HashMap::new(),
            nn: NN::new(&[5, 6, 1]),
            clock: 0,
        }
    }

    /// Touch a cache block.
    ///
    /// This should be called whenever the object `id` represents is used (read, written, etc.).
    ///
    /// This will train the neural network with the new data.
    pub fn touch(&mut self, id: Id) {
        {
            // Get the block we need.
            let block = self.blocks.get_mut(&id).unwrap();

            // Apply a bijective map from the clock to a float on the range (0,1), which can be
            // fed to the network.
            let goal = (self.clock as f64 * 0.01).tanh();
            // Train the neural network with the existing data against the clock.
            self.nn.train(&[(block.as_vec(id), vec![goal])]);

            // Update the block with last used data.
            block.last_used[0] = block.last_used[1];
            block.last_used[1] = self.clock;
            // Increment the frequency counter.
            block.times_used += 1;
        }

        // Tick the clock.
        self.tick();
    }

    /// Insert a new cache block into the cache tracker.
    pub fn insert(&mut self, id: Id) {
        self.blocks.insert(id, Block {
            last_used: [!0; 2],
            instated: self.clock,
            times_used: 0,
        });
    }

    /// Remove a cache block.
    pub fn remove(&mut self, id: Id) {
        self.blocks.remove(&id);
    }

    /// Get an iterator over blocks from cold to hot.
    pub fn cold(&mut self) -> ColdIter {
        // Build a heap over the predictions.
        let mut heap = BinaryHeap::new();
        for (&id, block) in self.blocks.iter() {
            // Predict the next use.
            let prediction = self.nn.run(&block.as_vec(id))[0];
            // Push the prediction to the heap.
            heap.push(Prediction {
                id: id,
                prediction: prediction,
            });
        }

        ColdIter {
            heap: heap,
        }
    }

    /// Get at iterator over blocks to remove to trim the cache tracker to `to`.
    ///
    /// Note that this won't remove the blocks, and this should be handled manually with the
    /// `remove` method.
    pub fn trim(&mut self, to: usize) -> ::std::iter::Take<ColdIter> {
        self.cold().take(self.blocks.len() - to)
    }
}

/// A cache operation.
enum CacheOperation {
    /// Create a new cache block with some ID.
    Insert(Id),
    /// Remove a cache block.
    Remove(Id),
    /// Touch some block.
    Touch(Id),
}

/// A concurrent cache tracker.
///
/// This has two parts to it:
///
/// - A normal cache tracker, protected by a lock.
/// - A queue of cache operations that will be executed when the lock is acquired.
pub struct ConcurrentCache {
    /// The inner cache tracker, protected by a lock.
    inner: Mutex<Cache>,
    /// The cache tracker operation queue.
    ///
    /// In order to avoid excessively locking and unlocking the cache tracker, we buffer the
    /// operations, which will then be executed in one go, when needed.
    queue: SegQueue<CacheOperation>,
}

impl ConcurrentCache {
    /// Create a new concurrent cache tracker.
    pub fn new() -> ConcurrentCache {
        ConcurrentCache {
            inner: Mutex::new(Cache::new()),
            queue: SegQueue::new(),
        }
    }

    /// Lock the inner cache.
    pub fn lock(&self) -> MutexGuard<Cache> {
        // Lock the cache tracker.
        let mut lock = self.inner.lock();
        // Commit the buffered operations to the tracker.
        while let Some(op) = self.queue.try_pop() {
            match op {
                CacheOperation::Insert(id) => lock.insert(id),
                CacheOperation::Remove(id) => lock.remove(id),
                CacheOperation::Touch(id) => lock.touch(id),
            }
        }

        lock
    }

    /// Insert a new cache block.
    pub fn insert(&mut self, id: Id) {
        self.queue.push(CacheOperation::Insert(id));
    }

    /// Remove a cache block.
    pub fn remove(&mut self, id: Id) {
        self.queue.push(CacheOperation::Remove(id));
    }

    /// Touch a cache block.
    pub fn touch(&mut self, id: Id) {
        self.queue.push(CacheOperation::Touch(id));
    }
}
