//! The global state.

use std::sync::{mpsc, Mutex};
use std::collections::HashSet;
use std::mem;
use {rand, hazard};
use garbage::Garbage;

lazy_static! {
    pub static ref STATE: State = State::new();
}

pub fn create_hazard() -> hazard::Writer {
    // Create the hazard.
    let (write, read) = hazard::create();
    // Communicate the new hazard to the global state through the channel.
    STATE.chan.send(Message::NewHazard(read));
    // Return the other half of the hazard.
    write
}

pub fn export_garbage(garbage: Vec<Garbage>) {
    STATE.chan.send(Message::Garbage(garbage));
    tick();
}

pub fn gc() {
    STATE.try_gc();
}

pub fn tick() {
    const GC_PROBABILITY: usize = (!0) / 64;

    if rand::random() < GC_PROBABILITY {
        gc();
    }
}

enum Message {
    Garbage(Vec<Garbage>),
    NewHazard(hazard::Reader),
}

struct State {
    chan: mpsc::Sender<Message>,
    garbo: Mutex<Garbo>,
}

impl State {
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

    fn try_gc(&self) {
        // Lock the "garbo" (the part of the state needed to GC).
        let garbo = self.garbo.lock();
        // Handle all the messages sent.
        garbo.handle_all();
        // Collect the garbage.
        garbo.gc();
    }
}

struct Garbo {
    chan: mpsc::Receiver<Message>,
    garbage: Vec<Garbage>,
    hazards: Vec<hazard::Reader>,
}

impl State {
    fn handle_all(&mut self) {
        // Pop messages one-by-one and handle them respectively.
        while let Ok(msg) = self.chan.try_recv() {
            self.handle(msg);
        }
    }

    fn handle(&mut self, msg: Message) {
        match msg {
            // Append the garbage bulk to the garbage list.
            Message::Garbage(garbage) => self.garbage.append(garbage),
            // Register the new hazard into the state.
            Message::NewHazard(hazard) => self.hazards.push(hazard),
        }
    }

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
