#[macro_use]
extern crate lazy_static;

mod boxed;
mod garbage;
mod global;
mod guard;
mod hazard;
mod local;

pub use boxed::Cell;
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
    global::gc():
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
