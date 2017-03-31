//! The TFS library.
//!
//! This is the official implementation of the TFS specification. It implements the specification
//! in its full form, and is accessible as a library.

#![feature(conservative_impl_trait, i128_type, try_from)]

#[macro_use]
extern crate slog;

extern crate cbloom;
extern crate crossbeam;
extern crate futures;
extern crate little_endian;
extern crate lz4_compress;
extern crate mlcr;
extern crate rand;
extern crate ring;
extern crate ring_pwhash;
extern crate seahash;
extern crate speck;
extern crate thread_object;
extern crate type_name;

#[macro_use]
mod error;
#[macro_use]
mod macros;

mod alloc;
mod disk;
mod fs;

pub use error::Error;
