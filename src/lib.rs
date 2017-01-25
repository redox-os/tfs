//! The TFS library.
//!
//! This is the official implementation of the TFS specification. It implements the specification
//! in its full form, and is accessible as a library.

#[macro_use]
extern crate slog;
#[macro_use]
extern crate quick_error;

mod macros;
mod io;
