//! Comparator-only custody coverage. Counts are not timings or authority.
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Counter {
    FreshKernel,
    ObserveKernel,
    Begin,
    BeginSkipped,
    Armed,
    Interrupted,
    ObserveFailed,
    End,
    EndArmed,
    DispatchWithdrawn,
    DispatchLive,
    DispatchDead,
    ExpiredFallback,
    ExhaustedFallback,
    Unregistered,
}

pub const NAMES: [&str; 15] = [
    "fresh_kernel",
    "observe_kernel",
    "begin",
    "begin_skipped",
    "armed",
    "interrupted",
    "observe_failed",
    "end",
    "end_armed",
    "dispatch_withdrawn",
    "dispatch_live",
    "dispatch_dead",
    "expired_fallback",
    "exhausted_fallback",
    "unregistered",
];
static COUNTS: [AtomicU64; NAMES.len()] = [const { AtomicU64::new(0) }; NAMES.len()];

pub fn add(counter: Counter) {
    COUNTS[counter as usize].fetch_add(1, Ordering::Relaxed);
}

/// Process-wide cumulative counts, independently sampled; not a transaction.
pub fn snapshot() -> [u64; NAMES.len()] {
    std::array::from_fn(|i| COUNTS[i].load(Ordering::Relaxed))
}

pub(super) fn record(output: &mut impl std::io::Write, sequence: u64) -> std::io::Result<()> {
    use std::io::Write;
    let before = super::nanos(rustix::time::ClockId::Monotonic) / 1000;
    let counts = snapshot();
    let after = super::nanos(rustix::time::ClockId::Monotonic) / 1000;
    let mut bytes = [0u8; super::PROGRESS_RECORD_BYTES];
    let mut buffer = std::io::Cursor::new(bytes.as_mut_slice());
    write!(
        buffer,
        "sophia_custody_dispatch schema=1 mode=progress process=session pid={} sequence={sequence} before_usec={before} after_usec={after}",
        std::process::id()
    )?;
    for (name, count) in NAMES.iter().zip(counts) {
        write!(buffer, " {name}={count}")?;
    }
    writeln!(buffer)?;
    let length = buffer.position() as usize;
    output.write_all(&bytes[..length])
}
