//! A safe wrapper around the `type_name` API.

#![no_std]
#![feature(core_intrinsics)]

use core::intrinsics;

/// Get the type name of `T`.
pub fn get<T>() -> &'static str {
    unsafe {
        intrinsics::type_name::<T>()
    }
}
