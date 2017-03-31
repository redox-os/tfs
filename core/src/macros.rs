/// Delegate logging to a field of a struct.
///
/// This implements `slog::Drain` for a type, by delegating the calls into some field of the type.
///
/// # Example
///
/// ```rust
/// delegate_log!(MyType.my_field);
/// ```
macro_rules! delegate_log {
    ($ty:ident.$field:ident) => {
        impl<E, L> ::slog::Drain for $ty<L>
        where L: ::slog::Drain<Error = E> {
            type Error = E;

            fn log(&self, info: &::slog::Record, o: &::slog::OwnedKeyValueList) -> Result<(), E> {
                // Redirect the call to the field.
                self.$field.log(info, o)
            }
        }
    }
}

/// Convenience macro for creating a future.
///
/// This creates a type `impl Future<Ok = T, Err = Error>` with `T` being the given argument.
// TODO: Eventually replace by type alias.
macro_rules! future {
    ($ok:ty) => {
        impl Future<Item = $ok, Error = $crate::Error>
    }
}
