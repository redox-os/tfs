
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
    ($type:ty.$field:ident) => {
        impl<L: slog::Drain> Drop for $type<L> {
            type Error = L::Error;

            fn log(&self, info: &slog::Record, o: &slog::OwnedKeyValueList) -> Result<(), L::Error> {
                // Redirect the call to the field.
                self.$field.log(info, o)
            }
        }
    }
}
