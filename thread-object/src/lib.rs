//! Thread-specific objects.
//!
//! This is an abstraction over usual thread-local storage, adding a special type which has a value
//! for every thread.
//!
//! This means that you can dynamically create TLS variables, as opposed to the classical fixed
//! static variable. This means that you can store the object reference in a struct, and have many
//! in the same thread.
//!
//! It works by holding a TLS variable with a binary tree map associating unique object IDs with
//! pointers to the object.
//!
//! Performance wise, this is suboptimal, but it is portable contrary to most other approaches.

#![feature(const_fn)]

use std::any::Any;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::mem;
use std::sync::atomic;

/// The ID counter.
///
/// This is incremented when a new object is created, associating an unique value with the object.
static ID_COUNTER: atomic::AtomicUsize = atomic::AtomicUsize::new(0);

thread_local! {
    /// This thread's thread object maps.
    ///
    /// This maps IDs to pointers to the associated object.
    static THREAD_OBJECTS: RefCell<BTreeMap<usize, Box<Any>>> = RefCell::new(BTreeMap::new());
}

/// A multi-faced object.
///
/// An initial value is chosen upon creation. This value will be copied once the thread reads it
/// for the first time. The value can be read and written, but will only be presented for the
/// current thread. As such, it is "many-faced" meaning that different threads view different
/// values.
#[derive(Copy, Clone)]
pub struct Object<T> {
    /// The initial value cloned when read by a new thread.
    initial: T,
    /// The ID of the object.
    id: usize,
}

impl<T> Object<T> {
    /// Create a new thread object with some initial value.
    ///
    /// The specified value `initial` will be the value assigned when new threads read the object.
    pub fn new(initial: T) -> Object<T> {
        Object {
            initial: initial,
            // Increment the ID counter and use the previous value. Relaxed ordering is fine as it
            // guarantees uniqueness, which is the only constraint we need.
            id: ID_COUNTER.fetch_add(1, atomic::Ordering::Relaxed),
        }
    }
}

impl<T: Clone + Any> Object<T> {
    /// Read and/or modify the value associated with this thread.
    ///
    /// This reads the object's value associated with the current thread, and initializes it if
    /// necessary. The mutable reference to the object is passed through the closure `f` and the
    /// return value of said closure is then returned.
    ///
    /// The reason we use a closure is to prevent the programmer leaking the pointer to another
    /// thread, causing memory safety issues as the pointer is only valid in the current thread.
    pub fn with<F, R>(&self, f: F) -> R
    where F: FnOnce(&mut T) -> R {
        // We'll fetch it from the thread object map.
        THREAD_OBJECTS.with(|map| {
            // TODO: Eliminate this `RefCell`.
            let mut guard = map.borrow_mut();
            // Fetch the pointer to the object, and initialize if it doesn't exist.
            let ptr = guard.entry(self.id).or_insert_with(|| Box::new(self.initial.clone()));
            // Run it through the provided closure.
            f(ptr.downcast_mut().unwrap())
        })
    }

    /// Replace the inner value.
    ///
    /// This replaces the inner value with `new` and returns the old value.
    pub fn replace(&self, new: T) -> T {
        self.with(|x| mem::replace(x, new))
    }

    /// Copy the inner value.
    pub fn get(&self) -> T
    where T: Copy {
        self.with(|x| *x)
    }
}

impl<T: Default> Default for Object<T> {
    fn default() -> Object<T> {
        Object::new(T::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::thread;
    use std::sync::{Mutex, Arc};

    #[test]
    fn initial_value() {
        let obj = Object::new(23);
        obj.with(|&mut x| assert_eq!(x, 23));
        assert_eq!(obj.with(|&mut x| x), 23);
    }

    #[test]
    fn string() {
        let obj = Object::new(String::new());

        obj.with(|x| {
            assert!(x.is_empty());

            x.push('b');
        });

        obj.with(|x| {
            assert_eq!(x, "b");

            x.push('a');
        });

        obj.with(|x| {
            assert_eq!(x, "ba");
        });
    }

    #[test]
    fn multiple_objects() {
        let obj1 = Object::new(0);
        let obj2 = Object::new(0);

        obj2.with(|x| *x = 1);

        obj1.with(|&mut x| assert_eq!(x, 0));
        obj2.with(|&mut x| assert_eq!(x, 1));
    }

    #[test]
    fn multi_thread() {
        let obj = Object::new(0);
        thread::spawn(move || {
            obj.with(|x| *x = 1);
        }).join().unwrap();

        obj.with(|&mut x| assert_eq!(x, 0));

        thread::spawn(move || {
            obj.with(|&mut x| assert_eq!(x, 0));
            obj.with(|x| *x = 2);
        }).join().unwrap();

        obj.with(|&mut x| assert_eq!(x, 0));
    }

    #[test]
    fn replace() {
        let obj = Object::new(420); // blaze it
        assert_eq!(obj.replace(42), 420);
        assert_eq!(obj.replace(32), 42);
        assert_eq!(obj.replace(0), 32);
    }

    #[test]
    fn default() {
        assert_eq!(Object::<usize>::default().get(), 0);
    }

    #[derive(Clone)]
    struct Dropper {
        is_dropped: Arc<Mutex<bool>>,
    }

    impl Drop for Dropper {
        fn drop(&mut self) {
            *self.is_dropped.lock().unwrap() = true;
        }
    }

    #[test]
    fn drop() {
        let is_dropped = Arc::new(Mutex::new(false));
        let arc = is_dropped.clone();
        thread::spawn(move || {
            let obj = Object::new(Dropper {
                is_dropped: arc,
            });

            obj.with(|_| {});

            mem::forget(obj);
        }).join().unwrap();

        assert!(*is_dropped.lock().unwrap());
    }
}
