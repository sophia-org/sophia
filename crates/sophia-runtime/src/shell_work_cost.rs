//! Opt-in attribution, never an authorization cache or a performance verdict.
//! CPU clocks and counters belong to one thread. Nested stages are inclusive.
//! Disabled calls still branch, but read no additional clock and emit nothing.

use std::{cell::RefCell, marker::PhantomData, rc::Rc, sync::OnceLock};

mod record;
mod state;
pub use record::valid_record;
use state::{Outcome, State, TurnStart};

#[cfg(test)]
#[path = "../tests/support/shell_work_cost.rs"]
pub(crate) mod tests;

pub(crate) const CLASSES: [&str; 4] = ["frame_demand", "candidate", "resource_retire", "other"];

static ENABLED: OnceLock<bool> = OnceLock::new();
static SINK: OnceLock<fn(&str)> = OnceLock::new();
thread_local! { static COST: RefCell<State> = RefCell::new(thread_state()); }

fn thread_state() -> State {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_THREAD: AtomicU64 = AtomicU64::new(1);
    let mut state = State::new(enabled());
    if state.enabled {
        match NEXT_THREAD.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1)) {
            Ok(id) => state.thread_sequence = id,
            Err(_) => state.saturations = 1,
        }
    }
    state
}

fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var("SOPHIA_SHELL_WORK_COST").is_ok_and(|v| v == "1"))
}

/// Install only the host's existing evidence printer, before workers start.
pub fn install_sink(sink: fn(&str)) {
    if enabled() {
        let _ = SINK.set(sink);
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub(crate) enum Stage {
    Copy,
    Allocate,
    Read,
    Validate,
    Pixels,
    ImageAlive,
    Admission,
    Reserve,
    Settle,
    SourceRelease,
    Retire,
    Collect,
    Remove,
    Custody,
}
pub(crate) const STAGES: [&str; 14] = [
    "copy",
    "allocate",
    "read",
    "validate",
    "pixels",
    "image_alive",
    "admission",
    "reserve",
    "settle",
    "source_release",
    "retire",
    "collect",
    "remove",
    "custody",
];

#[derive(Clone, Copy, Debug, Default)]
#[repr(usize)]
pub(crate) enum Boundary {
    #[default]
    Other,
    BeforeTurn,
    ServerTurn,
    ServerFlush,
    AfterTurn,
    FileCheck,
    ImageCheck,
    ImageAccept,
}
pub(crate) const BOUNDARIES: [&str; 8] = [
    "other",
    "before_turn",
    "server_turn",
    "server_flush",
    "after_turn",
    "file_check",
    "image_check",
    "image_accept",
];

#[derive(Clone, Copy)]
pub(crate) enum Call {
    ExportPoll,
    FileCheck,
    Custody,
    ImageAlive,
}

fn cpu() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    (t.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(t.tv_nsec as u64)
}

/// A guard cannot move to another thread or retain a borrow over the operation.
struct Timer {
    start: Option<u64>,
    stage: Stage,
    bytes: u64,
    outcome: Outcome,
    _thread: PhantomData<Rc<()>>,
}
impl Timer {
    fn new(stage: Stage, bytes: u64) -> Self {
        let start = COST.with(|s| s.borrow_mut().read(cpu));
        Self {
            start,
            stage,
            bytes,
            outcome: Outcome::Aborted,
            _thread: PhantomData,
        }
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            COST.with(|s| {
                let mut s = s.borrow_mut();
                let end = s.read(cpu).expect("enabled timer stays enabled");
                s.finish(self.stage, self.outcome, self.bytes, start, end);
            });
        }
    }
}

pub(crate) fn result<T, E>(
    stage: Stage,
    bytes: u64,
    run: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let mut timer = Timer::new(stage, bytes);
    let result = run();
    timer.outcome = if result.is_ok() {
        Outcome::Ok
    } else {
        Outcome::Error
    };
    result
}
pub(crate) fn option<T>(stage: Stage, run: impl FnOnce() -> Option<T>) -> Option<T> {
    let mut timer = Timer::new(stage, 0);
    let result = run();
    timer.outcome = if result.is_some() {
        Outcome::Ok
    } else {
        Outcome::Empty
    };
    result
}
pub(crate) fn unit(stage: Stage, run: impl FnOnce()) {
    let mut timer = Timer::new(stage, 0);
    run();
    timer.outcome = Outcome::Ok;
}

struct Tag(Option<Boundary>, PhantomData<Rc<()>>);
impl Drop for Tag {
    fn drop(&mut self) {
        if let Some(previous) = self.0 {
            COST.with(|s| s.borrow_mut().boundary = previous);
        }
    }
}
pub(crate) fn boundary<T>(at: Boundary, run: impl FnOnce() -> T) -> T {
    let old = COST.with(|s| {
        let mut s = s.borrow_mut();
        s.enabled.then(|| std::mem::replace(&mut s.boundary, at))
    });
    let _tag = Tag(old, PhantomData);
    run()
}
pub(crate) fn count(call: Call) {
    COST.with(|s| s.borrow_mut().call(call));
}

/// A transport service invocation, including failed and flush-only invocations.
pub(crate) fn turn<T>(flush: bool, run: impl FnOnce() -> T) -> T {
    struct Turn(Option<TurnStart>, bool);
    impl Drop for Turn {
        fn drop(&mut self) {
            if let Some(before) = self.0 {
                COST.with(|s| s.borrow_mut().turn(self.1, before));
            }
        }
    }
    let before = COST.with(|s| {
        let s = s.borrow();
        s.enabled.then(|| s.turn_start())
    });
    let _turn = Turn(before, flush);
    run()
}

/// Counts successful decoding, not acceptance, journal publication or 9P ops.
pub(crate) fn decode<T, E>(kind: usize, run: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    let result = run();
    COST.with(|s| s.borrow_mut().decode(kind, result.is_ok()));
    result
}

/// Already typed-decoded staged records; admission is successful Submitted custody.
pub(crate) fn record_class(kind: sophia_protocol::shell_files::ShellFileKind, admitted: bool) {
    use sophia_protocol::shell_files::ShellFileKind;
    let i = match kind {
        ShellFileKind::FrameDemand => 0,
        ShellFileKind::Candidate => 1,
        ShellFileKind::ResourceRetire => 2,
        _ => 3,
    };
    COST.with(|s| s.borrow_mut().class(i, admitted));
}

/// Server-produced permit committed to the journal, not read or presented.
pub(crate) fn published(kind: sophia_protocol::shell_files::ShellFileKind) {
    if kind == sophia_protocol::shell_files::ShellFileKind::FramePermit {
        COST.with(|s| s.borrow_mut().permit());
    }
}

/// Worker records use completed-job intervals, not the owner's frame interval.
pub(crate) fn job_completed(bytes: u64, succeeded: bool) {
    let emit = COST.with(|s| s.borrow_mut().job(bytes, succeeded));
    if emit {
        emit_snapshot(true, false);
    }
}
pub(crate) fn admission<T, E>(kind: usize, run: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    let result = run();
    COST.with(|s| s.borrow_mut().admission(kind, result.is_ok()));
    result
}

pub struct OwnerExit;
impl Drop for OwnerExit {
    fn drop(&mut self) {
        emit_snapshot(false, true);
    }
}

pub(crate) struct WorkerExit;
impl Drop for WorkerExit {
    fn drop(&mut self) {
        emit_snapshot(true, true);
    }
}

/// Each row in a snapshot has identical CPU/monotonic stamps and identity.
/// Invalid counters remain visible with valid=0; they must not be reduced.
pub fn emit_snapshot(worker: bool, final_record: bool) {
    if !enabled() {
        return;
    }
    let Some(sink) = SINK.get() else {
        return;
    };
    let snapshot = COST.with(|s| {
        let mut s = s.borrow_mut();
        let now = s.read(cpu).expect("enabled snapshot");
        s.snapshot();
        (s.clone(), now)
    });
    let stamp = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let monotonic = (stamp.tv_sec as u64)
        .saturating_mul(1_000_000)
        .saturating_add(stamp.tv_nsec as u64 / 1000);
    let tid = rustix::thread::gettid().as_raw_nonzero().get() as u64;
    for row in record::rows(
        &snapshot.0,
        snapshot.1,
        tid,
        monotonic,
        worker,
        final_record,
    ) {
        sink(&row);
    }
}
