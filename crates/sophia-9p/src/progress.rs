//! Comparison-only process totals. Core counts include every 9P server in the
//! process; they are not exclusive shell costs or wall-clock measurements.
use std::sync::atomic::{AtomicU64, Ordering};
pub mod service_ledger;
pub mod wait_sources;

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
    OwnerCompleted,
    OwnerSetup,
    OwnerRecord,
    OwnerSubmit,
    OwnerPixel,
    OwnerCadence,
    OwnerFlushOnly,
    OwnerOther,
    OwnerUnattributed,
    OwnerMixed,
}

pub const NAMES: [&str; 30] = [
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
    "owner_completed",
    "owner_setup",
    "owner_record",
    "owner_submit",
    "owner_pixel",
    "owner_cadence",
    "owner_flush_only",
    "owner_other",
    "owner_unattributed",
    "owner_mixed",
];
static TOTALS: [AtomicU64; 30] = [const { AtomicU64::new(0) }; 30];

/// No allocation, clock, formatting or I/O on the instrumented path. Wrapping
/// totals must be rejected by analysis rather than interpreted as progress.
pub fn add(counter: Counter, amount: u64) {
    if matches!(counter, Counter::WrittenBytes) {
        service_ledger::written(amount);
    }
    TOTALS[counter as usize].fetch_add(amount, Ordering::Relaxed);
    if amount != 0 {
        match counter {
            Counter::RequestWalk | Counter::RequestOpen => observe(Activity::Setup),
            Counter::RequestRead | Counter::RequestClunk | Counter::RequestWrite => {
                observe(Activity::Other)
            }
            Counter::WrittenBytes => observe(Activity::Flush),
            _ => {}
        }
    }
}

/// Independent monotonic observations, not a transaction across counters.
pub fn snapshot() -> [u64; 30] {
    std::array::from_fn(|i| TOTALS[i].load(Ordering::Relaxed))
}

/// Work observed on the owner thread, not the cause of its wake-up. Categories
/// may overlap; other 9P roles contribute setup/read/flush activity as well.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Activity {
    Setup,
    Record,
    Submit,
    Pixel,
    Cadence,
    Other,
    Flush,
}
thread_local! {
    static PASS: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}
pub fn observe(activity: Activity) {
    PASS.with(|pass| {
        if let Some(bits) = pass.get() {
            pass.set(Some(bits | (1 << activity as u8)));
        }
    });
}

/// Thread-local and allocation-free. Drop records early returns and continues;
/// the sampler can observe a started pass before it has completed.
pub struct OwnerPass(std::marker::PhantomData<*mut ()>);
pub fn owner_pass() -> OwnerPass {
    PASS.with(|pass| assert!(pass.replace(Some(0)).is_none(), "nested owner pass"));
    add(Counter::OwnerPass, 1);
    OwnerPass(std::marker::PhantomData)
}
impl Drop for OwnerPass {
    fn drop(&mut self) {
        let bits = PASS.with(|pass| pass.replace(None).expect("active owner pass"));
        add(Counter::OwnerCompleted, 1);
        let work = bits & 0x3f;
        for (i, counter) in [
            Counter::OwnerSetup,
            Counter::OwnerRecord,
            Counter::OwnerSubmit,
            Counter::OwnerPixel,
            Counter::OwnerCadence,
            Counter::OwnerOther,
        ]
        .into_iter()
        .enumerate()
        {
            if work & (1 << i) != 0 {
                add(counter, 1);
            }
        }
        if work.count_ones() > 1 {
            add(Counter::OwnerMixed, 1);
        }
        if bits == 0 {
            add(Counter::OwnerUnattributed, 1);
        } else if work == 0 {
            add(Counter::OwnerFlushOnly, 1);
        }
    }
}
