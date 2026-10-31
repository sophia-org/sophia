use super::{BOUNDARIES, Boundary, CLASSES, Call, STAGES, Stage};

#[derive(Clone, Copy)]
pub(super) enum Outcome {
    Ok,
    Error,
    Empty,
    Aborted,
}
#[derive(Clone, Default)]
pub(super) struct StageCost {
    pub calls: u64,
    pub errors: u64,
    pub empty: u64,
    pub aborted: u64,
    pub cpu: u64,
    pub maximum: u64,
    pub bytes: u64,
    pub ok_bytes: u64,
}
#[derive(Clone, Default)]
pub(super) struct Calls {
    pub poll: u64,
    pub check: u64,
    pub custody: u64,
    pub image_alive: u64,
}
#[derive(Clone, Default)]
pub(super) struct ClassCost {
    pub decoded: u64,
    pub admitted: u64,
    pub records: [u64; 2],
    pub turns: [u64; 2],
}
#[derive(Clone, Copy)]
pub(super) struct TurnStart {
    decoded: u64,
    classes: [u64; CLASSES.len()],
}
#[derive(Clone)]
pub(super) struct State {
    pub thread_sequence: u64,
    pub enabled: bool,
    pub boundary: Boundary,
    pub stages: [StageCost; STAGES.len()],
    pub calls: [Calls; BOUNDARIES.len()],
    pub reads: u64,
    pub regressions: u64,
    pub saturations: u64,
    pub first: Option<u64>,
    pub last: Option<u64>,
    pub snapshots: u64,
    pub turns: [u64; 2],
    pub empty_turns: [u64; 2],
    pub turn_records: [u64; 2],
    pub decodes: [u64; 3],
    pub decode_errors: [u64; 3],
    pub decoded_total: u64,
    pub classes: [ClassCost; CLASSES.len()],
    pub server_frame_permits: u64,
    pub admissions: [u64; 3],
    pub admission_errors: [u64; 3],
    pub jobs: u64,
    pub job_errors: u64,
    pub job_bytes: u64,
}

fn add(value: &mut u64, delta: u64, saturations: &mut u64) {
    *value = value.checked_add(delta).unwrap_or_else(|| {
        *saturations = saturations.saturating_add(1);
        u64::MAX
    });
}
impl State {
    pub fn new(enabled: bool) -> Self {
        Self {
            thread_sequence: 0,
            enabled,
            boundary: Boundary::Other,
            stages: std::array::from_fn(|_| StageCost::default()),
            calls: std::array::from_fn(|_| Calls::default()),
            reads: 0,
            regressions: 0,
            saturations: 0,
            first: None,
            last: None,
            snapshots: 0,
            turns: [0; 2],
            empty_turns: [0; 2],
            turn_records: [0; 2],
            decodes: [0; 3],
            decode_errors: [0; 3],
            decoded_total: 0,
            classes: std::array::from_fn(|_| ClassCost::default()),
            server_frame_permits: 0,
            admissions: [0; 3],
            admission_errors: [0; 3],
            jobs: 0,
            job_errors: 0,
            job_bytes: 0,
        }
    }
    pub fn read(&mut self, clock: impl FnOnce() -> u64) -> Option<u64> {
        if !self.enabled {
            return None;
        }
        add(&mut self.reads, 1, &mut self.saturations);
        let now = clock();
        if self.last.is_some_and(|last| now < last) {
            add(&mut self.regressions, 1, &mut self.saturations);
        }
        self.first.get_or_insert(now);
        self.last = Some(now);
        Some(now)
    }
    pub fn finish(&mut self, stage: Stage, outcome: Outcome, bytes: u64, from: u64, to: u64) {
        let s = &mut self.stages[stage as usize];
        add(&mut s.calls, 1, &mut self.saturations);
        let elapsed = to.saturating_sub(from);
        add(&mut s.cpu, elapsed, &mut self.saturations);
        s.maximum = s.maximum.max(elapsed);
        add(&mut s.bytes, bytes, &mut self.saturations);
        match outcome {
            Outcome::Ok => add(&mut s.ok_bytes, bytes, &mut self.saturations),
            Outcome::Error => add(&mut s.errors, 1, &mut self.saturations),
            Outcome::Empty => add(&mut s.empty, 1, &mut self.saturations),
            Outcome::Aborted => add(&mut s.aborted, 1, &mut self.saturations),
        }
    }
    pub fn call(&mut self, call: Call) {
        if !self.enabled {
            return;
        }
        let s = &mut self.calls[self.boundary as usize];
        add(
            match call {
                Call::ExportPoll => &mut s.poll,
                Call::FileCheck => &mut s.check,
                Call::Custody => &mut s.custody,
                Call::ImageAlive => &mut s.image_alive,
            },
            1,
            &mut self.saturations,
        );
    }
    pub fn decoded(&self) -> u64 {
        self.decoded_total
    }
    pub fn decode(&mut self, i: usize, ok: bool) {
        if !self.enabled {
            return;
        }
        add(&mut self.decodes[i], 1, &mut self.saturations);
        if !ok {
            add(&mut self.decode_errors[i], 1, &mut self.saturations);
        } else {
            add(&mut self.decoded_total, 1, &mut self.saturations);
        }
    }
    pub fn admission(&mut self, i: usize, ok: bool) {
        if !self.enabled {
            return;
        }
        add(&mut self.admissions[i], 1, &mut self.saturations);
        if !ok {
            add(&mut self.admission_errors[i], 1, &mut self.saturations);
        }
    }
    pub fn turn_start(&self) -> TurnStart {
        TurnStart {
            decoded: self.decoded(),
            classes: std::array::from_fn(|i| self.classes[i].decoded),
        }
    }
    pub fn class(&mut self, i: usize, admitted: bool) {
        if !self.enabled {
            return;
        }
        let class = &mut self.classes[i];
        add(
            if admitted {
                &mut class.admitted
            } else {
                &mut class.decoded
            },
            1,
            &mut self.saturations,
        );
    }
    pub fn permit(&mut self) {
        if self.enabled {
            add(&mut self.server_frame_permits, 1, &mut self.saturations);
        }
    }
    pub fn turn(&mut self, flush: bool, before: TurnStart) {
        let i = usize::from(flush);
        let decoded = self.decoded().saturating_sub(before.decoded);
        for (class, before) in self.classes.iter_mut().zip(before.classes) {
            let count = class.decoded.saturating_sub(before);
            add(&mut class.records[i], count, &mut self.saturations);
            if count != 0 {
                add(&mut class.turns[i], 1, &mut self.saturations);
            }
        }
        add(&mut self.turns[i], 1, &mut self.saturations);
        add(&mut self.turn_records[i], decoded, &mut self.saturations);
        if decoded == 0 {
            add(&mut self.empty_turns[i], 1, &mut self.saturations);
        }
    }
    pub fn job(&mut self, bytes: u64, ok: bool) -> bool {
        if !self.enabled {
            return false;
        }
        add(&mut self.jobs, 1, &mut self.saturations);
        add(&mut self.job_bytes, bytes, &mut self.saturations);
        if !ok {
            add(&mut self.job_errors, 1, &mut self.saturations);
        }
        self.jobs.is_multiple_of(256)
    }
    pub fn snapshot(&mut self) {
        add(&mut self.snapshots, 1, &mut self.saturations);
    }
    pub fn valid(&self) -> bool {
        self.regressions == 0 && self.saturations == 0
    }
}
