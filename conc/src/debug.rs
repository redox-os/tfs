//! Runtime debugging tools.

/// Execute closure when the environment variable, `CONC_DEBUG_MODE`, is set.
///
/// When compiled in release mode, this is a NOP.
#[cfg(debug_assertions)]
pub fn exec<F: FnOnce()>(f: F) {
    use std::env;

    thread_local! {
        /// Is `CONC_DEBUG_MODE` set?
        ///
        /// This is cached to avoid expensive repeated syscalls or similar things.
        static IS_ENABLED: bool = env::var("CONC_DEBUG_MODE").is_ok();
    }

    // If enabled, run the closure.
    if IS_ENABLED.with(|&x| x) {
        f();
    }
}

/// Do nothing.
///
/// When compiled in debug mode, this will execute the closure when envvar `CONC_DEBUG_MODE` is
/// set.
#[inline]
#[cfg(not(debug_assertions))]
pub fn exec<F: FnOnce()>(_: F) {}
