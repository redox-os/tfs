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
//! Epoch-based reclamation has some unfortunate issues. It cannot work properly if an epoch is
//! constantly active. It assumes that at some point, the thread reads no objects, which is true in
//! some cases, but not always. This makes it unsuitable for many thing, such as event loops and
//! more.
//!
//! Futhermore, to end an epoch, the system must do some relatively expensive operations, whereas
//! `concurrent` (and most hazard pointer implementations) need not to do this, as it can reuse
//! the "epochs" (in this case "hazards") later.
//!
//! While I have no benchmarks yet, the tests I've made (on skiplists and other structures)
//! generally shows that `concurrent` outperforms `crossbeam` in most cases.
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

#[macro_use]
extern crate lazy_static;
extern crate rand;

mod cell;
mod garbage;
mod global;
mod guard;
mod hazard;
mod local;

pub use cell::Cell;
pub use guard::Guard;

use std::mem;

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
    local::add_garbage(ptr, mem::transmute(dtor));
}
