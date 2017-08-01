//! Runtime debugging tools.

#[cfg(feature = "debug-tools")]
extern crate backtrace;

/// Execute closure when the environment variable, `CONC_DEBUG_MODE`, is set.
///
/// When compiled in release mode, this is a NOP.
#[cfg(feature = "debug-tools")]
pub fn exec<F: FnOnce()>(f: F) {
    use self::backtrace::Backtrace;
    use std::env;

    thread_local! {
        /// Is `CONC_DEBUG_MODE` set?
        ///
        /// This is cached to avoid expensive repeated syscalls or similar things.
        static DEBUG_MODE_ENABLED: bool = env::var("CONC_DEBUG_MODE").is_ok();
        /// Is `CONC_DEBUG_STACKTRACE` set?
        ///
        /// This is cached to avoid expensive repeated syscalls or similar things.
        static STACK_TRACE_ENABLED: bool = env::var("CONC_DEBUG_STACKTRACE").is_ok();
    }

    // If enabled, run the closure.
    if DEBUG_MODE_ENABLED.with(|&x| x) {
        f();
        if STACK_TRACE_ENABLED.with(|&x| x) {
            println!("{:?}", Backtrace::new());
        }
    }
}

/// Do nothing.
///
/// When compiled in debug mode, this will execute the closure when envvar `CONC_DEBUG_MODE` is
/// set.
#[inline]
#[cfg(not(feature = "debug-tools"))]
pub fn exec<F: FnOnce()>(_: F) {}
