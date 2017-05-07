//! # `concurrent` — An efficient concurrent reclamation system
//!
//! `concurrent` builds upon hazard pointers to create a extremely performant system for
//! concurrently handling memory. It is more general and convinient — and often also faster — than
//! epoch-based reclamation.
//!
//! ## Why?
//!
//! aturon's [blog post](https://aturon.github.io/blog/2015/08/27/epoch/) explains the issues of
//! concurrent memory handling very well, although it take basis in epoch-based reclamation, which
//! this crate is an alternative for.
//!
//! The gist essentially is that you need to delete objects in most concurrent data structure
//! (otherwise there would be memory leaks), however cannot safely do so, as there is no way to
//! know if another thread is accessing the object in question. This (and other reclamation
//! systems) provides a solution to this problem.
//!
//! ## Usage
//!
//! While the low-level API is available, it is generally sufficient to use the `concurrent::Cell`
//! abstraction. This acts much like familiar Rust APIs. It allows the programmer to concurrently
//! access a value through references, as well as update it, and more. Refer to the respective docs
//! for more information.
//!
//! ## Why not crossbeam/epochs?
//!
//! Epochs and classical hazard pointers are generally faster than this crate, but it doesn't
//! matter how fast it is, it has to be right.
//!
//! The issue with most other and faster solutions is that, if there is a non-trivial amount of
//! threads (say 16) constantly reading/storing some pointer, it will never get to a state, where
//! it can be reclaimed.
//!
//! In other words, given sufficient amount of threads and frequency, the gaps between the
//! reclamation might be very very long, causing very high memory usage, and potentially OOM
//! crashes.
//!
//! These issues are not hypothethical. It happened to me while testing the caching system of TFS.
//! Essentially, the to-be-destroyed garbage accumulated several gigabytes, without ever being open
//! to a collection cycle.
//!
//! It reminds of the MongoDB debate. It might very well be the fastest solution¹, but if it can't
//! even ensure consistency, what is the point?
//!
//! That being said, there are cases where this library is faster than the alternatives.
//! Moreover, there are cases where the other libraries are fine (e.g. if you have a bounded number
//! of thread and a medium-long interval between accesses).
//!
//! ¹If you want a super fast memory reclamation system, you should try NOP™, and not calling
//!  destructors.
//!
//! ## Internals
//!
//! It based on hazard pointers, although there are several differences. The idea is essentially
//! that the system keeps track of some number of "hazards". As long as a hazard protects some
//! object, the object cannot be deleted.
//!
//! Once in a while, a thread performs a garbage collection by scanning the hazards and finding the
//! objects not currently protected by any hazard. These objects are then deleted.
//!
//! To improve performance, we use a layered approach: Both garbage (objects to be deleted
//! eventually) and hazards are cached thread locally. This reduces the amount of atomic operations
//! and cache misses.
//!
//! ## Garbage collection
//!
//! Garbage collection of the concurrently managed object is done automatically between every `n`
//! frees where `n` is chosen from some probability distribution.
//!
//! Note that a garbage collection cycle might not clear all objects. For example, some objects
//! could be protected by hazards. Others might not have been exported from the thread-local cache
//! yet.
//!
//! ## Performance
//!
//! It is worth noting that atomic reads through this library usually requires three atomic CPU
//! instruction, this means that if you are traversing a list or something like that, this library
//! might not be for you.

#[macro_use]
extern crate lazy_static;
extern crate parking_lot;
extern crate rand;

mod cell;
mod garbage;
mod global;
mod guard;
mod hazard;
mod local;
mod mpsc;

pub use cell::Cell;
pub use guard::Guard;

use std::mem;
use garbage::Garbage;

/// Collect garbage.
///
/// This function does two things:
///
/// 1. Export garbage from current thread to the global queue.
/// 2. Collect all the garbage and run destructors on the unused items.
///
/// If another thread is currently doing 2., it will be skipped.
///
/// # Use case
///
/// Note that it is not necessary to call this manually, it will do so automatically after some
/// time has passed.
///
/// However, it can be nice if you have just trashed a very memory-hungy item in the current
/// thread, and want to attempt to GC it.
///
/// # Other threads
///
/// This cannot collect un-propagated garbage accumulated locally in other threads. This will only
/// attempt to collect the accumulated local and global (propagated) garbage.
pub fn gc() {
    // Export the local garbage to ensure that the garbage of the current thread gets collected.
    local::export_garbage();
    // Run the global GC.
    global::gc();
}

/// Declare a pointer unreachable garbage to be deleted eventually.
///
/// This adds `ptr` to the queue of garbage, which eventually will be destroyed through its
/// destructor given in `dtor`. This is ensured to happen at some point _after_ the last guard
/// protecting the pointer is dropped.
///
/// It is legal for `ptr` to be invalidated by `dtor`, such that accessing it is undefined after
/// `dtor` has been run. This means that `dtor` can safely (there are exceptions, see below) run a
/// destructor of `ptr`'s data.
///
/// # Unreachability criterion
///
/// If you invalidate `ptr` in the destructor, it is extremely important that `ptr` is no longer
/// reachable from any data structure: It should be impossible to create _new_ guard representing
/// `ptr` from now on, as such thing can mean that new guards can be created after it is dropped
/// causing use-after-free.
pub fn add_garbage<T>(ptr: &'static T, dtor: fn(&'static T)) {
    local::add_garbage(unsafe {
        Garbage::new(ptr as *const T as *const u8 as *mut u8, mem::transmute(dtor))
    });
}
