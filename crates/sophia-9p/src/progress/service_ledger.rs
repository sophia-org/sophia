//! Diagnostic service ledger: bounded counters, no clock or allocation.
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

pub const NAMES: [&str; 17] = [
    "wire_owner_input",
    "wire_standalone_input",
    "wire_flush_useful",
    "wire_flush_no_progress",
    "wire_flush_failed",
    "wire_flush_blocked",
    "wire_flush_deferred",
    "wire_no_publication",
    "retry_wake_ready",
    "retry_wake_error",
    "retry_wake_pending",
    "retry_write_ready",
    "retry_write_error",
    "retry_write_pending",
    "retry_other_ready",
    "retry_other_error",
    "retry_other_pending",
];
static COUNTS: [AtomicU64; 17] = [const { AtomicU64::new(0) }; 17];
thread_local! {
    static OWNER: Cell<bool> = const { Cell::new(false) };
    static RETRY: Cell<usize> = const { Cell::new(2) };
    static WRITTEN: Cell<u64> = const { Cell::new(0) };
}
pub fn add(index: usize) {
    COUNTS[index].fetch_add(1, Ordering::Relaxed);
}
pub fn snapshot() -> [u64; 17] {
    std::array::from_fn(|i| COUNTS[i].load(Ordering::Relaxed))
}
pub struct OwnerInput(bool, std::marker::PhantomData<*mut ()>);
pub fn owner_input() -> OwnerInput {
    OwnerInput(OWNER.replace(true), std::marker::PhantomData)
}
impl Drop for OwnerInput {
    fn drop(&mut self) {
        OWNER.set(self.0);
    }
}
pub fn input() {
    add(usize::from(!OWNER.get()));
}
pub struct RetrySource(usize, std::marker::PhantomData<*mut ()>);
pub fn retry_source(source: usize) -> RetrySource {
    assert!(source < 3);
    RetrySource(RETRY.replace(source), std::marker::PhantomData)
}
impl Drop for RetrySource {
    fn drop(&mut self) {
        RETRY.set(self.0);
    }
}
/// Ready=0, error=1, pending=2. Caller identity comes from the scoped driver.
pub fn retry_outcome(outcome: usize) {
    add(8 + RETRY.get() * 3 + outcome);
}
pub fn written(bytes: u64) {
    WRITTEN.set(WRITTEN.get().wrapping_add(bytes));
}
pub fn written_bytes() -> u64 {
    WRITTEN.get()
}
pub fn flush(before: u64, failed: bool) {
    add(if failed {
        4
    } else if WRITTEN.get() != before {
        2
    } else {
        3
    });
}
