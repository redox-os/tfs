//! The global state.

use parking_lot::Mutex;
use std::collections::HashSet;
use std::mem;
use {rand, hazard, mpsc};
use garbage::Garbage;

lazy_static! {
    /// The global state.
    ///
    /// This state is shared between all the threads.
    pub static ref STATE: State = State::new();
}

/// Create a new hazard.
///
/// This creates a new hazard and registers it in the global state. It's secondary, writer part is
/// returned.
pub fn create_hazard() -> hazard::Writer {
    // Create the hazard.
    let (write, read) = hazard::create();
    // Communicate the new hazard to the global state through the channel.
    STATE.chan.send(Message::NewHazard(read));
    // Return the other half of the hazard.
    write
}

/// Export garbage into the global state.
///
/// This adds the garbage, which will eventually be destroyed, to the global state.
pub fn export_garbage(garbage: Vec<Garbage>) {
    // Send the garbage to the message-passing channel of the state.
    STATE.chan.send(Message::Garbage(garbage));
    // Tick the clock, potentially triggering a garbage collection.
    tick();
}

/// Atempt to garbage collect.
///
/// If another garbage collection is currently running, nothing will happen.
pub fn gc() {
    STATE.try_gc();
}

/// Tick the clock.
///
/// This shall be called when new garbage is added, as it will trigger a GC by some probability.
pub fn tick() {
    /// The probabiity of triggering a GC.
    ///
    /// This probability is given such that `0` corresponds to 0 and `!0` corresponds to `1`.
    const GC_PROBABILITY: usize = (!0) / 64;

    // Generate a random number and compare it against the probability.
    if rand::random() <= GC_PROBABILITY {
        // The outfall was to GC.
        gc();
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

    /// Try to collect the garbage.
    ///
    /// This will handle all of the messages in the channel and then attempt at collect the
    /// garbage. If another thread is currently collecting garbage, it will be equivalent to NOP.
    ///
    /// Garbage collection works by scanning the hazards and dropping all the garbage which is not
    /// currently active in the hazards.
    fn try_gc(&self) {
        // Lock the "garbo" (the part of the state needed to GC).
        if let Ok(garbo) = self.garbo.try_lock() {
            // Handle all the messages sent.
            garbo.handle_all();
            // Collect the garbage.
            garbo.gc();
        }
    }
}

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
            Message::Garbage(garbage) => self.garbage.append(garbage),
            // Register the new hazard into the state.
            Message::NewHazard(hazard) => self.hazards.push(hazard),
        }
    }

    /// Receive and handle all the messages.
    fn handle_all(&mut self) {
        // Go over every message.
        self.chan.recv_all(|msg| {
            self.handle(msg);
        });
    }

    /// Garbage collect all unused garbage.
    fn gc(&mut self) {
        // Create the set which will keep the _active_ hazards.
        let mut active = HashSet::with_capacity(self.hazards.len());

        // Take out the hazards and go over them one-by-one.
        for hazard in mem::replace(self.hazards, Vec::with_capacity(self.hazards.len())) {
            match hazard.get() {
                // The hazard is dead, so the other end (the writer) is not available anymore,
                // hence we can safely destroy it.
                hazard::State::Dead => unsafe { hazard.destroy() },
                // The hazard is free and must thus be put back to the hazard list.
                hazard::State::Free => self.hazards.push(hazard),
                hazard::State::Active(ptr) => {
                    // This hazard is active, hence we insert the pointer it contains in our
                    // "active" set.
                    active.insert(ptr);
                    // Since the hazard is still alive, we must put it back to the hazard list for
                    // future use.
                    self.hazards.push(hazard);
                },
            }
        }

        // Take the garbage and scan it for unused garbage.
        for garbage in mem::replace(self.garbage, Vec::new()) {
            if active.contains(garbage.ptr) {
                // If the garbage is in the set of active pointers, it will be put back to the
                // garbage list.
                self.garbage.push(garbage);
            } else {
                // The garbage is unused and not referenced by any hazard, hence we can safely
                // destroy it.
                unsafe { garbage.destroy(); }
            }
        }
    }
}
