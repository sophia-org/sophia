//! Comparator-only shell-export attempts and owner-pass co-occurrence.
//! Counts do not imply successful operations, wake causes, or elapsed cost.
use std::cell::Cell;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

pub const NAMES: [&str; 6] = [
    "record",
    "submit",
    "ack",
    "event_ready",
    "event_pending",
    "event_error",
];
#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Operation {
    Record,
    Submit,
    Ack,
    EventReady,
    EventPending,
    EventError,
}
static SCOPED: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
static UNSCOPED: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
static MASKS: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];
thread_local! { static PASS: Cell<Option<u8>> = const { Cell::new(None) }; }

pub(super) fn begin() {
    assert!(PASS.replace(Some(0)).is_none());
}
pub(super) fn end() {
    let mask = PASS.replace(None).expect("exchange owner scope");
    MASKS[mask as usize].fetch_add(1, Ordering::Relaxed);
}
pub fn observe(operation: Operation) {
    let index = operation as usize;
    let counts = if let Some(mask) = PASS.get() {
        PASS.set(Some(mask | (1 << index)));
        &SCOPED
    } else {
        &UNSCOPED
    };
    counts[index].fetch_add(1, Ordering::Relaxed);
}
/// Independent atomic reads, not a transaction across fields.
#[derive(Debug)]
pub struct Snapshot {
    pub scoped: [u64; 6],
    pub unscoped: [u64; 6],
    pub masks: [u64; 64],
}
pub fn snapshot() -> Snapshot {
    Snapshot {
        scoped: std::array::from_fn(|i| SCOPED[i].load(Ordering::Relaxed)),
        unscoped: std::array::from_fn(|i| UNSCOPED[i].load(Ordering::Relaxed)),
        masks: std::array::from_fn(|i| MASKS[i].load(Ordering::Relaxed)),
    }
}
impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, name) in NAMES.iter().enumerate() {
            write!(
                f,
                " scoped_{name}={} unscoped_{name}={}",
                self.scoped[i], self.unscoped[i]
            )?;
        }
        for (mask, count) in self.masks.iter().enumerate() {
            if *count != 0 {
                write!(f, " mask_{mask}={count}")?;
            }
        }
        Ok(())
    }
}
