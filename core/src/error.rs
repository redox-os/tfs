/// The category of an error.
///
/// This enum contains variants representing general categories of TFS errors.
#[derive(PartialEq)]
pub enum Kind {
    /// Data corruption.
    Corruption,
    /// No more space to use.
    OutOfSpace,
    /// Implementation issue.
    Implementation,
}

/// A TFS error.
#[derive(PartialEq)]
pub struct Error {
    /// The type ("kind") of the error.
    pub kind: Kind,
    /// Description of the error.
    desc: Box<str>,
}

/// Create a TFS error.
///
/// This constructs a value of type `Error` defined by the given parameter.
///
/// The first argument defines the kind (`Kind`) of the error. There is no need for importing the
/// type, as it is already prefixed with the enum.
///
/// The rest arguments are the usual formatting syntax (like `println!()`) representing the
/// `Display` implementation of the error. If none, it will simply use the second argument (the
/// description).
#[macro_export]
macro_rules! err {
    ($kind:ident, $($rest:tt)*) => {
        $crate::error::Error {
            kind: $crate::error::Kind::$kind,
            desc: format!($($rest)*),
        }
    };
}
