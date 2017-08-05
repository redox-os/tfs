//! Settings and presets.

use std::cell::Cell;

thread_local! {
    /// The settings for the current thread.
    static LOCAL_SETTINGS: Cell<Settings> = Cell::new(Settings::default())
}

/// Settings for the system.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Settings {
    /// The probability of triggering a GC when ticking.
    ///
    /// Whenever the system "ticks" it generates a random number. If the number is below this
    /// setting, it will try to collect the garbage.
    ///
    /// So, this probability is given such that `0` corresponds to never and `!0` corresponds to
    /// nearly always.
    pub gc_probability: usize,
    /// The maximal amount of garbage before exportation to the global state.
    ///
    /// When the local state's garbage queue exceeds this limit, it exports it to the global
    /// garbage queue.
    pub max_garbage_before_export: usize,
    /// The maximal amount of non-free hazards in the thread-local cache.
    ///
    /// When it exceeds this limit, it will clean up the cached hazards. With "cleaning up" we mean
    /// setting the state of the hazards to "free" in order to allow garbage collection of the
    /// object it is currently protecting.
    pub max_non_free_hazards: usize,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            gc_probability: (!0) / 128,
            max_garbage_before_export: 64,
            max_non_free_hazards: 16,
        }
    }
}

impl Settings {
    /// Preset for low memory, high CPU usage.
    pub fn low_memory() -> Settings {
        Settings {
            gc_probability: (!0) / 32,
            max_garbage_before_export: 16,
            max_non_free_hazards: 4,
        }
    }

    /// Preset for high memory, low CPU usage.
    pub fn low_cpu() -> Settings {
        Settings {
            gc_probability: (!0) / 256,
            max_garbage_before_export: 128,
            max_non_free_hazards: 32,
        }
    }

    /// Disable GC for this settings instance.
    ///
    /// This ensures that the current thread will not be blocked to collect garbage. The garbage
    /// can still be propagated and destroyed, it will just not happen in this thread.
    pub fn disable_automatic_gc(&mut self) {
        self.gc_probability = 0;
    }

    /// Disable automatic exportation.
    ///
    /// This ensures that no destructors gets exported to the global state before the thread exits.
    /// In particular, no destructor will be run unless exportation is explicitly done.
    pub fn disable_automatic_export(&mut self) {
        // Set to the max value. This will prevent exportation, as the garbage (which is of more
        // than one byte) queue would have to fill more than the whole memory space, which is
        // obviously impossible.
        self.max_garbage_before_export = !0;
    }
}

/// Get the settings of the current thread.
pub fn get() -> Settings {
    LOCAL_SETTINGS.with(|x| x.get())
}

/// Set the settings for the current thread.
///
/// # Important
///
/// This is not global. That is, if you call this in thread A, the setting change won't affect
/// thread B. If you want to have the same settings in multiple threads, you should call this
/// function in the start of every thread you spawn with the `Settings`, you want.
pub fn set_local(settings: Settings) {
    LOCAL_SETTINGS.with(|x| x.set(settings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use {Garbage, local};

    #[test]
    fn set_get() {
        set_local(Settings {
            max_garbage_before_export: 22,
            .. Default::default()
        });
        assert_eq!(get().max_garbage_before_export, 22);
    }

    #[test]
    fn default() {
        thread::spawn(|| {
            assert_eq!(get(), Settings::default());
        }).join().unwrap();
    }

    #[test]
    fn disable_automatic_gc() {
        thread_local! {
            static X: Cell<bool> = Cell::default();
        }

        fn dtor(_: *const u8) {
            X.with(|x| x.set(true));
        }

        let mut settings = get();
        settings.disable_automatic_gc();
        set_local(settings);

        for _ in 0..100000 {
            local::add_garbage(Garbage::new(0x1 as *const u8, dtor));
            assert!(!X.with(|x| x.get()));
        }

        // Avoid messing with other tests.
        set_local(Settings::default());
    }

    #[test]
    fn disable_automatic_exportation() {
        fn dtor(x: *const u8) {
            unsafe {
                *(x as *mut u8) = 1;
            }
        }

        let mut settings = get();
        settings.disable_automatic_export();
        set_local(settings);

        for _ in 0..100000 {
            let b = Box::new(0);
            local::add_garbage(Garbage::new(&*b, dtor));
            assert_eq!(*b, 0);
        }

        // Avoid messing with other tests.
        set_local(Settings::default());
    }

    #[test]
    fn compare_presets() {
        let low = Settings::low_memory();
        let high = Settings::low_cpu();

        assert!(low.gc_probability > high.gc_probability);
        assert!(high.max_garbage_before_export > low.max_garbage_before_export);
        assert!(high.max_non_free_hazards > low.max_non_free_hazards);
    }
}
