use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

use rustix::time::{ClockId, clock_gettime};

use super::{Reason, Role, Snapshot, Stage, StageCost};

static ENABLED: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static THREADS: AtomicU64 = AtomicU64::new(0);
static QUEUE: Mutex<Vec<Snapshot>> = Mutex::new(Vec::new());

thread_local! { static STATE: RefCell<State> = RefCell::new(State::open()); }

/// Turn timers on or off for every thread; sampled at each timer entry.
pub fn enable(on: bool) {
    ENABLED.store(on, Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Relaxed)
}

/// Start a new generation. The calling thread publishes its previous
/// generation now; other threads publish when they next enter a timer or exit.
pub fn begin_block() -> u64 {
    // 2^64 blocks cannot happen in one process; a wrap would alias generations.
    let generation = GENERATION.fetch_add(1, Relaxed).wrapping_add(1);
    let _ = STATE.try_with(|state| state.borrow_mut().observe());
    generation
}

/// Take every snapshot published by generation rolls and thread exits.
pub fn drain() -> Vec<Snapshot> {
    QUEUE
        .lock()
        .map(|mut queue| std::mem::take(&mut *queue))
        .unwrap_or_default()
}

/// Peek at the calling thread without resetting it.
pub fn snapshot_thread() -> Option<Snapshot> {
    STATE
        .try_with(|state| {
            let mut state = state.borrow_mut();
            state.observe();
            state.finished(Reason::Explicit)
        })
        .ok()
}

/// Set the calling thread's role and open its outer window here.
pub fn register_thread(role: Role) {
    let _ = STATE.try_with(|state| {
        let mut state = state.borrow_mut();
        state.snapshot.role = role;
        state.reopen(GENERATION.load(Relaxed));
    });
}

/// Run `f` inside an inclusive `stage` scope. The thread-local borrow is
/// released before `f` runs, so scopes nest; the scope is recorded on return
/// and on unwind.
pub fn measure<T>(stage: Stage, f: impl FnOnce() -> T) -> T {
    let on = ENABLED.load(Relaxed);
    let scope = STATE
        .try_with(|state| {
            let mut state = state.borrow_mut();
            state.observe();
            if on {
                let snapshot = &mut state.snapshot;
                bump(&mut snapshot.enabled_calls, &mut snapshot.invalid);
                state.active = state.active.saturating_add(1);
                let start = clocks();
                state.snapshot.invalid |= start.is_none();
                Some(Scope {
                    stage,
                    generation: state.snapshot.generation,
                    start,
                    done: false,
                })
            } else {
                let snapshot = &mut state.snapshot;
                bump(&mut snapshot.disabled_calls, &mut snapshot.invalid);
                None
            }
        })
        .ok()
        .flatten();
    let out = f();
    if let Some(mut scope) = scope {
        scope.done = true;
    }
    out
}

struct Scope {
    stage: Stage,
    generation: u64,
    start: Option<(u64, u64)>,
    done: bool,
}

impl Drop for Scope {
    fn drop(&mut self) {
        let _ = STATE.try_with(|state| {
            state
                .borrow_mut()
                .close(self.stage, self.generation, self.start, !self.done)
        });
    }
}

struct State {
    snapshot: Snapshot,
    /// Open timed scopes on this thread.
    active: u32,
}

impl State {
    fn open() -> State {
        let tid = u32::try_from(rustix::thread::gettid().as_raw_nonzero().get()).unwrap_or(0);
        let mut state = State {
            active: 0,
            snapshot: Snapshot {
                role: Role::Owner,
                reason: Reason::Explicit,
                tid,
                thread_sequence: THREADS.fetch_add(1, Relaxed),
                generation: 0,
                cpu_first_nsec: 0,
                cpu_now_nsec: 0,
                mono_first_nsec: 0,
                mono_now_nsec: 0,
                enabled_calls: 0,
                disabled_calls: 0,
                stages: [StageCost::default(); Stage::COUNT],
                clock_regressions: 0,
                saturations: 0,
                invalid: false,
            },
        };
        state.reopen(GENERATION.load(Relaxed));
        state
    }

    /// Reset counters and open the outer window now, keeping thread identity.
    fn reopen(&mut self, generation: u64) {
        let snapshot = &mut self.snapshot;
        let now = clocks();
        let (cpu, mono) = now.unwrap_or((0, 0));
        snapshot.generation = generation;
        snapshot.cpu_first_nsec = cpu;
        snapshot.cpu_now_nsec = cpu;
        snapshot.mono_first_nsec = mono;
        snapshot.mono_now_nsec = mono;
        snapshot.enabled_calls = 0;
        snapshot.disabled_calls = 0;
        snapshot.stages = [StageCost::default(); Stage::COUNT];
        snapshot.clock_regressions = 0;
        snapshot.saturations = 0;
        snapshot.invalid = now.is_none();
    }

    fn finished(&self, reason: Reason) -> Snapshot {
        let mut snapshot = self.snapshot.clone();
        snapshot.reason = reason;
        match clocks() {
            Some((cpu, mono)) => {
                snapshot.cpu_now_nsec = cpu;
                snapshot.mono_now_nsec = mono;
                snapshot.invalid |=
                    cpu < snapshot.cpu_first_nsec || mono < snapshot.mono_first_nsec;
            }
            None => snapshot.invalid = true,
        }
        snapshot
    }

    /// Publish the current generation if a newer one exists, then reset. An
    /// open scope straddling the change voids both generations.
    fn observe(&mut self) {
        let generation = GENERATION.load(Relaxed);
        if generation != self.snapshot.generation {
            self.snapshot.invalid |= self.active > 0;
            publish(self.finished(Reason::Block));
            self.reopen(generation);
            self.snapshot.invalid |= self.active > 0;
        }
    }

    /// Record a scope opened in `generation`. A scope that straddles a
    /// generation change, whether this thread already rolled or the global
    /// generation moved while the scope was open, voids the block instead of
    /// merging clocks across the boundary.
    fn close(&mut self, stage: Stage, generation: u64, start: Option<(u64, u64)>, aborted: bool) {
        self.active = self.active.saturating_sub(1);
        let snapshot = &mut self.snapshot;
        let cost = &mut snapshot.stages[stage as usize];
        bump(&mut cost.calls, &mut snapshot.invalid);
        if aborted {
            bump(&mut cost.aborted, &mut snapshot.invalid);
        }
        let (Some((cpu_then, mono_then)), Some((cpu_now, mono_now))) = (start, clocks()) else {
            snapshot.invalid = true;
            return;
        };
        if generation != snapshot.generation || generation != GENERATION.load(Relaxed) {
            snapshot.invalid = true;
            return;
        }
        let cost = &mut snapshot.stages[stage as usize];
        let cpu = accumulate(
            cpu_then,
            cpu_now,
            &mut cost.cpu_nsec,
            &mut cost.max_cpu_nsec,
        );
        let elapsed = accumulate(
            mono_then,
            mono_now,
            &mut cost.elapsed_nsec,
            &mut cost.max_elapsed_nsec,
        );
        for fault in [cpu, elapsed].into_iter().flatten() {
            snapshot.invalid = true;
            match fault {
                Fault::Regression => {
                    snapshot.clock_regressions = snapshot.clock_regressions.saturating_add(1)
                }
                Fault::Saturation => snapshot.saturations = snapshot.saturations.saturating_add(1),
            }
        }
    }
}

impl Drop for State {
    /// Publishes under the generation this thread last observed, so a thread
    /// that finished its work before the owner began the next block still
    /// reports to the block it worked in; its outer window ends at exit.
    fn drop(&mut self) {
        publish(self.finished(Reason::Drop));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    Regression,
    Saturation,
}

/// Add `now - then` to `total` and raise `max`; report what invalidates the block.
pub(crate) fn accumulate(then: u64, now: u64, total: &mut u64, max: &mut u64) -> Option<Fault> {
    let Some(delta) = now.checked_sub(then) else {
        return Some(Fault::Regression);
    };
    *max = (*max).max(delta);
    match total.checked_add(delta) {
        Some(sum) => {
            *total = sum;
            None
        }
        None => {
            *total = u64::MAX;
            Some(Fault::Saturation)
        }
    }
}

/// Saturating counter increment that voids the block on saturation.
pub(crate) fn bump(counter: &mut u64, invalid: &mut bool) {
    match counter.checked_add(1) {
        Some(next) => *counter = next,
        None => *invalid = true,
    }
}

fn publish(snapshot: Snapshot) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.push(snapshot);
    }
}

/// `(thread CPU, monotonic)` in nanoseconds; `None` when either read fails.
fn clocks() -> Option<(u64, u64)> {
    Some((nsec(ClockId::ThreadCPUTime)?, nsec(ClockId::Monotonic)?))
}

fn nsec(clock: ClockId) -> Option<u64> {
    let stamp = clock_gettime(clock);
    u64::try_from(stamp.tv_sec)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(stamp.tv_nsec).ok()?)
}
