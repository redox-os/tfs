//! The global state.

use parking_lot::Mutex;
use std::collections::HashSet;
use std::{mem, panic};
use {rand, hazard, mpsc, debug, settings};
use garbage::Garbage;

lazy_static! {
    /// The global state.
    ///
    /// This state is shared between all the threads.
    static ref STATE: State = State::new();
}

/// Create a new hazard.
///
/// This creates a new hazard and registers it in the global state. It's secondary, writer part is
/// returned.
pub fn create_hazard() -> hazard::Writer {
    STATE.create_hazard()
}

/// Export garbage into the global state.
///
/// This adds the garbage, which will eventually be destroyed, to the global state. Note that this
/// does not tick, and thus cannot cause garbage collection.
pub fn export_garbage(garbage: Vec<Garbage>) {
    STATE.export_garbage(garbage)
}

/// Attempt to garbage collect.
///
/// If another garbage collection is currently running, the thread will do nothing, and `Err(())`
/// will be returned. Otherwise, it returns `Ok(())`.
///
/// # Panic
///
/// If a destructor panics, this will panic as well.
pub fn try_gc() -> Result<(), ()> {
    STATE.try_gc()
}

/// Tick the clock.
///
/// This shall be called when new garbage is added, as it will trigger a GC by some probability.
pub fn tick() {
    // Generate a random number and compare it against the probability.
    if rand::random::<usize>() < settings::get().gc_probability {
        // The outfall was to (attempt at) GC.
        let _ = try_gc();
    }
}

/// A message to the global state.
enum Message {
    /// Add new garbage.
    Garbage(Vec<Garbage>),
    /// Add a new hazard.
    NewHazard(hazard::Reader),
}

/// The global state.
///
/// The global state is shared between all threads and keeps track of the garbage and the active
/// hazards.
///
/// It is divided into two parts: The channel and the garbo. The channel buffers messages, which
/// will eventually be executed at garbo, which holds all the data structures and is protected by a
/// mutex. The garbo holds the other end to the channel.
struct State {
    /// The message-passing channel.
    chan: mpsc::Sender<Message>,
    /// The garbo part of the state.
    garbo: Mutex<Garbo>,
}

impl State {
    /// Initialize a new state.
    fn new() -> State {
        // Create the message-passing channel.
        let (send, recv) = mpsc::channel();

        // Construct the state from the two halfs of the channel.
        State {
            chan: send,
            garbo: Mutex::new(Garbo {
                chan: recv,
                garbage: Vec::new(),
                hazards: Vec::new(),
            })
        }
    }

    /// Create a new hazard.
    ///
    /// This creates a new hazard and registers it in the global state. It's secondary, writer part
    /// is returned.
    fn create_hazard(&self) -> hazard::Writer {
        // Create the hazard.
        let (writer, reader) = hazard::create();
        // Communicate the new hazard to the global state through the channel.
        self.chan.send(Message::NewHazard(reader));
        // Return the other half of the hazard.
        writer
    }

    /// Export garbage into the global state.
    ///
    /// This adds the garbage, which will eventually be destroyed, to the global state.
    fn export_garbage(&self, garbage: Vec<Garbage>) {
        // Send the garbage to the message-passing channel of the state.
        self.chan.send(Message::Garbage(garbage));
    }

    /// Try to collect the garbage.
    ///
    /// This will handle all of the messages in the channel and then attempt at collect the
    /// garbage. If another thread is currently collecting garbage, `Err(())` is returned,
    /// otherwise it returns `Ok(())`.
    ///
    /// Garbage collection works by scanning the hazards and dropping all the garbage which is not
    /// currently active in the hazards.
    fn try_gc(&self) -> Result<(), ()> {
        // Lock the "garbo" (the part of the state needed to GC).
        if let Some(mut garbo) = self.garbo.try_lock() {
            // Collect the garbage.
            garbo.gc();

            Ok(())
        } else {
            // Another thread is collecting.
            Err(())
        }
    }
}

impl panic::RefUnwindSafe for State {}

/// The garbo part of the state.
///
/// This part is supposed to act like the garbage collecting part. It handles hazards, garbage, and
/// the receiving point of the message-passing channel.
struct Garbo {
    /// The channel of messages.
    chan: mpsc::Receiver<Message>,
    /// The to-be-destroyed garbage.
    garbage: Vec<Garbage>,
    /// The current hazards.
    hazards: Vec<hazard::Reader>,
}

impl Garbo {
    /// Handle a given message.
    ///
    /// "Handle" in this case refers to applying the operation defined by the message to the state,
    /// effectually executing the instruction of the message.
    fn handle(&mut self, msg: Message) {
        match msg {
            // Append the garbage bulk to the garbage list.
            Message::Garbage(mut garbage) => self.garbage.append(&mut garbage),
            // Register the new hazard into the state.
            Message::NewHazard(hazard) => self.hazards.push(hazard),
        }
    }

    /// Handle all the messages and garbage collect all unused garbage.
    ///
    /// # Panic
    ///
    /// If a destructor panics, this will panic as well.
    fn gc(&mut self) {
        // Print message in debug mode.
        debug::exec(|| println!("Collecting garbage."));

        // Handle all the messages sent.
        for msg in self.chan.recv_all() {
            self.handle(msg);
        }

        // Create the set which will keep the _active_ hazards.
        let mut active = HashSet::with_capacity(self.hazards.len());

        // Take out the hazards and go over them one-by-one.
        let len = self.hazards.len(); // TODO: This should be substituted into next line.
        for hazard in mem::replace(&mut self.hazards, Vec::with_capacity(len)) {
            match hazard.get() {
                // The hazard is dead, so the other end (the writer) is not available anymore,
                // hence we can safely destroy it.
                hazard::State::Dead => unsafe { hazard.destroy() },
                // The hazard is free and must thus be put back to the hazard list.
                hazard::State::Free => self.hazards.push(hazard),
                hazard::State::Protect(ptr) => {
                    // This hazard is active, hence we insert the pointer it contains in our
                    // "active" set.
                    active.insert(ptr);
                    // Since the hazard is still alive, we must put it back to the hazard list for
                    // future use.
                    self.hazards.push(hazard);
                },
            }
        }

        // Scan the garbage for unused objects.
        self.garbage.retain(|garbage| active.contains(&garbage.ptr()))
    }
}

impl Drop for Garbo {
    fn drop(&mut self) {
        // Do a final GC.
        self.gc();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garbage::Garbage;
    use std::{panic, ptr};

    #[test]
    fn dtor_runs() {
        fn dtor(x: *const u8) {
            unsafe {
                *(x as *mut u8) = 1;
            }
        }

        let s = State::new();
        for _ in 0..1000 {
            let b = Box::new(0);
            let h = s.create_hazard();
            h.protect(&*b);
            s.export_garbage(vec![Garbage::new(&*b, dtor)]);
            while s.try_gc().is_err() {}
            assert_eq!(*b, 0);
            while s.try_gc().is_err() {}
            h.free();
            while s.try_gc().is_err() {}
            assert_eq!(*b, 1);
            h.kill();
        }
    }

    #[test]
    fn clean_up_state() {
        fn dtor(x: *const u8) {
            unsafe {
                *(x as *mut u8) = 1;
            }
        }

        for _ in 0..1000 {
            let b = Box::new(0);
            {
                let s = State::new();
                s.export_garbage(vec![Garbage::new(&*b, dtor)]);
            }

            assert_eq!(*b, 1);
        }
    }

    #[test]
    fn panic_invalidate_state() {
        fn panic(_: *const u8) {
            panic!();
        }

        fn dtor(x: *const u8) {
            unsafe {
                *(x as *mut u8) = 1;
            }
        }

        let s = State::new();
        let b = Box::new(0);
        let h = create_hazard();
        h.protect(&*b);
        s.export_garbage(vec![Garbage::new(&*b, dtor), Garbage::new(0x2 as *const u8, panic)]);
        let _ = panic::catch_unwind(|| {
            while s.try_gc().is_err() {}
        });
        assert_eq!(*b, 0);
        h.free();
        while s.try_gc().is_err() {}
        assert_eq!(*b, 1);
    }

    #[test]
    #[should_panic]
    fn panic_in_dtor() {
        fn dtor(_: *const u8) {
            panic!();
        }

        let s = State::new();
        s.export_garbage(vec![Garbage::new(ptr::null(), dtor)]);
        while s.try_gc().is_err() {}
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn debug_more_hazards() {
        let s = State::new();
        let h = s.create_hazard();
        h.free();
        mem::forget(h);
    }
}
