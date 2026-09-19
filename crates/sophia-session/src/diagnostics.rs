//! Bounded, host-owned daily-session evidence. Proof archives have a separate owner.
pub mod application;
mod capture;
mod commands;
mod failure;
mod recovery;
mod shell_action;
mod shell_component;
pub use recovery::SessionCompletionFailure;
mod layout_epoch;
mod session_failure;
mod storage;
mod supervise;
mod x_lifecycle;
pub use supervise::supervise;

pub use capture::{
    Capture, DIAGNOSTIC_RECORD_MAX_BYTES, capture_line, capture_process_identity, recording,
    reduced_record,
};
pub use commands::{Inspection, Marker, Retention, SessionRecord, Store};
pub use failure::failure_code;
pub use session_failure::{SessionFailurePhase, session_failure_record};

use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const METADATA_LIMIT: u64 = 1024 * 1024;
pub(super) const SEGMENT_LIMIT: u64 = 15 * 1024 * 1024;
pub(super) const SEGMENTS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stamp {
    pub utc_msec: u64,
    pub boot_msec: u64,
}

impl Stamp {
    pub fn now() -> Self {
        let clock = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
        Self {
            utc_msec: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
            boot_msec: (clock.tv_sec as u64)
                .saturating_mul(1000)
                .saturating_add(clock.tv_nsec as u64 / 1_000_000),
        }
    }
}

pub(super) fn boot_id() -> io::Result<String> {
    Ok(std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .to_owned())
}

pub(super) fn start_ticks(pid: u32) -> io::Result<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let tail = stat
        .rsplit_once(')')
        .ok_or_else(|| storage::invalid("invalid process identity"))?
        .1;
    // Field 22; the command name in parentheses may itself contain spaces.
    let mut fields = tail.split_whitespace();
    if matches!(fields.next(), Some("Z" | "X")) {
        return Err(storage::invalid("session owner has exited"));
    }
    fields
        .nth(18)
        .map(str::to_owned)
        .ok_or_else(|| storage::invalid("missing process start identity"))
}

pub fn executable_digest(path: &std::path::Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}
