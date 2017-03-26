//! Convenient macro for creating I/O errors.
//!
//! This adds a macro, `err!()` which is used to create `std::io::Error` values. Refer to the
//! documentation of the macro for usage.

use std::{error, fmt};

/// Not used in public.
#[doc(hidden)]
#[derive(Debug)]
pub struct Err {
    /// Description of the error.
    pub desc: &'static str,
    /// The formatted string of the error.
    pub fmt: String,
}

impl fmt::Display for Err {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}", self.fmt)
    }
}

impl error::Error for Err {
    fn description(&self) -> &str {
        self.desc
    }
}

/// Create an I/O error.
///
/// This constructs a value of type `std::io::Error` defined by the given parameter.
///
/// The first argument defines the kind (`std::io::ErrorKind`) of the error. There is no need for
/// importing the type, as it is already prefixed with the enum.
///
/// The second argument is the description of the error, given in the form of a string literal.
///
/// The rest arguments are the usual formatting syntax (like `println!()`) representing the
/// `Display` implementation of the error. If none, it will simply use the second argument (the
/// description).
///
/// # Example
///
/// ```rust
/// let x = 42 + 3;
/// let error = err!(NotFound, "my error description", "this is an error, x is {}", x);
/// let error2 = err!(InvalidData, "my second error description");
/// ```
macro_rules! err {
    ($kind:ident, $desc:expr, $($rest:tt)*) => {
        // Construct the I/O error.
        ::std::io::Error::new(::std::io::ErrorKind::$kind, $crate::Err {
            desc: $desc,
            fmt: format!($($rest)*),
        })
    };
    // If the formatter is excluded, we default to the description.
    ($kind:ident, $desc:expr) => {
        err!($kind, $desc, "{}", $desc)
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn test() {
        let _ = err!(NotFound, "test");
        let _ = err!(NotFound, "test", "x {} y", 2 + 2);
    }
}
