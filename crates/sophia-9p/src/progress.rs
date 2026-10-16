//! Comparison-only process totals. Core counts include every 9P server in the
//! process; they are not exclusive shell costs or wall-clock measurements.
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Counter {
    OwnerPass,
    ComponentPass,
    ServerPollTurn,
    ServerFlush,
    HandoffAccepted,
    HandoffFallback,
    ReadBytes,
    WrittenBytes,
    RepliesEncoded,
    PendingReadAttempt,
    PeerContentPoll,
    PeerWait,
    RelayDispatch,
    RelayIdle,
    RelayExpiry,
    RequestWalk,
    RequestOpen,
    RequestRead,
    RequestWrite,
    RequestClunk,
}

pub const NAMES: [&str; 20] = [
    "owner_pass",
    "component_pass",
    "server_poll_turn",
    "server_flush",
    "handoff_accepted",
    "handoff_fallback",
    "read_bytes",
    "written_bytes",
    "replies_encoded",
    "pending_read_attempt",
    "peer_content_poll",
    "peer_wait",
    "relay_dispatch",
    "relay_idle",
    "relay_expiry",
    "request_walk",
    "request_open",
    "request_read",
    "request_write",
    "request_clunk",
];
static TOTALS: [AtomicU64; 20] = [const { AtomicU64::new(0) }; 20];

/// No allocation, clock, formatting or I/O on the instrumented path. Wrapping
/// totals must be rejected by analysis rather than interpreted as progress.
pub fn add(counter: Counter, amount: u64) {
    TOTALS[counter as usize].fetch_add(amount, Ordering::Relaxed);
}

/// Independent monotonic observations, not a transaction across counters.
pub fn snapshot() -> [u64; 20] {
    std::array::from_fn(|i| TOTALS[i].load(Ordering::Relaxed))
}
