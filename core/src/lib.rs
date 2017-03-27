//! The TFS library.
//!
//! This is the official implementation of the TFS specification. It implements the specification
//! in its full form, and is accessible as a library.

#![feature(conservative_impl_trait, i128_type)]

#[macro_use]
extern crate slog;

extern crate crossbeam;
extern crate futures;
extern crate little_endian;
extern crate lz4_compress;
extern crate ring;
extern crate seahash;
extern crate speck;

#[macro_use]
mod error;
#[macro_use]
mod macros;

mod alloc;
mod disk;
mod fs;
