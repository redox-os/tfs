use {global, hazard};

thread_local! {
    static STATE: State = State::default();
}

pub fn add_garbage(garbage: Garbage) {
    STATE.add_garbage(garbage)
}

pub fn get_hazard() -> hazard::Writer {
    STATE.get_hazard()
}

pub fn free_hazard(hazard: hazard::Writer) {
    STATE.free_hazard(hazard)
}

#[derive(Default)]
struct State {
    garbage: Vec<Garbage>,
    available_hazards: Vec<hazard::Writer>,
    available_hazards_free_after: usize,
}

impl State {
    fn non_free_hazards(&self) -> usize {
        self.available_hazard.len() - self.available_hazards_free_after
    }

    fn get_hazard(&mut self) -> hazard::Writer {
        if let Some(hazard) = self.available_hazards.pop() {
            hazard.block();
            hazard
        } else {
            global::get_hazard()
        }
    }

    fn free_hazard(&mut self, hazard: hazard::Writer) {
        const MAX_NON_FREE_HAZARDS: usize = 128;
        const MAX_HAZARDS: usize = 512;

        self.available_hazards.push(hazard);

        if self.non_free_hazards() > MAX_NON_FREE_HAZARDS {
            for i in &self.available_hazards[self.available_hazards_free_after..] {
                i.set(hazard::State::Free);
            }

            self.available_hazards_free_after = self.available_hazards_free_after.len();
        }
    }

    fn add_garbage(&mut self, garbage: Garbage) {
        const MAX_GARBAGE: usize = 64;

        self.garbage.push(garbage);

        // TODO: use memory instead of items as a metric.
        if self.garbage.len() > MAX_GARBAGE {
            self.transport_garbage();
        }
    }

    fn transport_garbage(&mut self) {
        global::transport_garbage(mem::replace(self.garbage, Vec::new()));
    }
}
