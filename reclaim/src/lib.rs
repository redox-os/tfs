#[macro_use]
extern crate lazy_static;

mod atomic;
mod garbage;
mod global;
mod guard;
mod hazard;
mod local;

pub use atomic::Atomic;
pub use guard::Guard;

pub fn gc() {
    // Export the local garbage to ensure that the garbage of the current thread gets collected.
    local::export_garbage();
    // Run the global GC.
    global::gc():
}

pub fn add_garbage<T>(ptr: &T, dtor: fn(&T)) {
    local::add_garbage(ptr, mem::transmute(dtor));
}
