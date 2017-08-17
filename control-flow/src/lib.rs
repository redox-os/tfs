//! A hack to control control-flow outside closures.
//!
//! This crate allows one to do things like breaking loops outside a closure. It works through a
//! a macro hack. Unless you really really need this, don't use it.
//!
//! # Example
//!
//! ```rust
//! #[macro_use]
//! extern crate control_flow;
//!
//! loop {
//!     let closure = || {
//!         defer!(break)
//!     };
//!
//!     // Breaks the loop.
//!     run_loop!(closure());
//! }
//! ```

/// A deferred control-flow command.
#[must_use = "Without using the `Command` it doesn't do anything. You should execute it through `run!()` or `run_loop!()`."]
pub enum Command<R, T> {
    /// Pass the value on.
    ///
    /// This is not the same as return. What it does is that instead of breaking the control flow,
    /// it passes on the value. That is, when `run!()` is called on this variant, the value that it
    /// holds is evaluated to.
    Give(T),
    /// Return the value.
    ///
    /// This (when eventually executed) returns the given value.
    Return(R),
    /// Break a loop.
    ///
    /// This (when eventually executed) breaks the loop.
    Break,
    /// Continue a loop.
    ///
    /// This (when eventually executed) continues the loop to next iteration.
    Continue,
}

/// Create a deferred control-flow command.
///
/// This takes a command (e.g. `return value`, `break`, `continue`, etc.) and creates the command
/// in the form of the `Command` enum. This is deferred (that is, it is not runned instantly) until
/// one executes the `Command`, which is done through `run!()` and `run_loop!()` depending on
/// whether or not you are in a loop.
#[macro_export]
macro_rules! defer {
    (return $val:expr) => { $crate::Command::Return($val) };
    (return) => { defer!(return ()) };
    (break) => { $crate::Command::Break };
    (continue) => { $crate::Command::Continue };
    ($val:expr) => { $crate::Command::Give($val) };
    () => { defer!(()) }
}

/// Run a deferred control-flow command (outside a loop).
///
/// This takes a `Command` and runs it. This only works when not using loop-specific commands.
#[macro_export]
macro_rules! run {
    ($command:expr) => {
        match $command {
            $crate::Command::Give(x) => x,
            $crate::Command::Return(x) => return x,
            _ => panic!("\
                Using loop-dependent `Command` variants without loop mode enabled. Consider using \
                `control_loop` instead.\
            "),
        }
    }
}

/// Run a deferred control-flow command within a loop.
///
/// This takes a `Command` and runs it.
#[macro_export]
macro_rules! run_loop {
    ($command:expr) => {
        match $command {
            $crate::Command::Give(x) => x,
            $crate::Command::Return(x) => return x,
            $crate::Command::Break => break,
            $crate::Command::Continue => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn loop_break() {
        let mut x = true;
        loop {
            run_loop!(defer!(break));
            x = false;
        }
        assert!(x);
    }

    #[test]
    fn loop_continue() {
        let mut x = true;
        for _ in 0..100 {
            assert!(x);
            run_loop!(defer!(continue));
            x = false;
        }
    }

    #[test]
    #[allow(unused_assignments)]
    fn return_early() {
        let x = false;
        run!(defer!(return));
        assert!(x);
    }

    #[test]
    #[allow(unused_assignments)]
    fn store_ctrl() {
        assert!((|| {
            let mut x = defer!(return false);
            x = defer!(return true);

            run!(x);
            unreachable!();
        })());
    }


    #[test]
    fn direct_value() {
        assert!(run!(defer!(true)));
        assert_eq!(run!(defer!()), ());
    }
}
