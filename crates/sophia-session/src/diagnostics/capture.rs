//! A bounded producer queue keeps diagnostic disk I/O off the session owner loop.
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::storage::Directory;
use super::{METADATA_LIMIT, SEGMENT_LIMIT, SEGMENTS, Stamp};

mod budget;

use budget::{Admission, NAME_SEGMENT_SHARE, SegmentBudget, budget_name};

const QUEUE_CAPACITY: usize = 256;
pub const DIAGNOSTIC_RECORD_MAX_BYTES: usize = 4096;

static SINK: OnceLock<Sink> = OnceLock::new();
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

struct Sink {
    sender: SyncSender<Event>,
    priority: SyncSender<Event>,
    identities: SyncSender<IdentityJob>,
    discarded: Arc<AtomicU64>,
}

enum Event {
    Line(Stamp, String),
}

struct IdentityJob(Stamp, &'static str, u64, Option<File>);

pub struct Capture {
    stop: Arc<AtomicBool>,
    finished: mpsc::Receiver<()>,
}

pub fn recording() -> bool {
    SINK.get().is_some()
}

/// Returns true when the installed capture owns presentation, including when
/// a full queue discards this line. Callers must not fall back to an unbounded log.
pub fn capture_line(line: &str) -> bool {
    let Some(sink) = SINK.get() else {
        return false;
    };
    if line.len() > DIAGNOSTIC_RECORD_MAX_BYTES {
        sink.discarded.fetch_add(1, Ordering::Relaxed);
        return true;
    }
    if let Some(line) = reduced_record(line) {
        send(sink, Event::Line(Stamp::now(), line));
    }
    true
}

/// Pin the executed inode while the supervised peer is known to be alive.
/// The PID is host control state; only the role, epoch and digest are persisted.
pub fn capture_process_identity(role: &'static str, pid: u32, epoch: u64) {
    let Some(sink) = SINK.get() else {
        return;
    };
    if !matches!(role, "wm" | "shell" | "sophia") {
        return;
    }
    let file = File::open(format!("/proc/{pid}/exe")).ok();
    let stamp = Stamp::now();
    send(
        sink,
        Event::Line(
            stamp,
            format!(
                "sophia_session_component schema=1 role={role} epoch={epoch} digest=unavailable status=pending"
            ),
        ),
    );
    if sink
        .identities
        .try_send(IdentityJob(stamp, role, epoch, file))
        .is_err()
    {
        sink.discarded.fetch_add(1, Ordering::Relaxed);
    }
}

fn identity_record(line: &str) -> bool {
    [
        "sophia_live_desktop_profile ",
        "sophia_session_component ",
        "sophia_session_profile ",
        "sophia_config_reload ",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

fn send(sink: &Sink, event: Event) {
    let Event::Line(_, line) = &event;
    let sender = if identity_record(line) {
        &sink.priority
    } else {
        &sink.sender
    };
    if matches!(
        sender.try_send(event),
        Err(TrySendError::Full(_) | TrySendError::Disconnected(_))
    ) {
        sink.discarded.fetch_add(1, Ordering::Relaxed);
    }
}

impl Capture {
    pub fn start(path: &Path) -> io::Result<Self> {
        let _install = INSTALL_LOCK
            .lock()
            .map_err(|_| io::Error::other("capture installation lock poisoned"))?;
        if SINK.get().is_some() {
            return Err(io::Error::other("session capture already installed"));
        }
        let directory = Directory::open(path, false)?;
        let discarded = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (priority_tx, priority_rx) = mpsc::sync_channel(32);
        let (identity_tx, identity_rx) = mpsc::sync_channel::<IdentityJob>(8);
        let hash_stop = stop.clone();
        std::thread::Builder::new().name("session-identities".into()).spawn(move || {
            while !hash_stop.load(Ordering::Acquire) {
                let Ok(IdentityJob(stamp, role, epoch, file)) = identity_rx.recv_timeout(Duration::from_millis(100)) else { continue; };
                let digest = file.and_then(|mut file| {
                    let mut hasher = Sha256::new();
                    io::copy(&mut file, &mut hasher).ok()?;
                    Some(format!("{:x}", hasher.finalize()))
                }).unwrap_or_else(|| "unavailable".into());
                if let Some(sink) = SINK.get() {
                    send(sink, Event::Line(stamp, format!("sophia_session_component schema=1 role={role} epoch={epoch} digest={digest} status=complete")));
                }
            }
        })?;
        let (finished_tx, finished) = mpsc::channel();
        SINK.set(Sink {
            sender,
            priority: priority_tx,
            identities: identity_tx,
            discarded: discarded.clone(),
        })
        .map_err(|_| io::Error::other("session capture already installed"))?;
        let worker_stop = stop.clone();
        std::thread::Builder::new()
            .name("session-records".into())
            .spawn(move || {
                let mut sequence = 0u64;
                let mut rotated = 0u64;
                let mut errors = 0u64;
                let mut budget = SegmentBudget::default();
                let mut synced = Instant::now();
                loop {
                    let event = match priority_rx
                        .try_recv()
                        .map_err(|_| mpsc::RecvTimeoutError::Timeout)
                        .or_else(|_| receiver.recv_timeout(Duration::from_millis(100)))
                    {
                        Ok(event) => Some(event),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let empty = event.is_none();
                    let result = (|| -> io::Result<()> {
                        let _lock = directory.lock()?;
                        if let Some(event) = event {
                            let (stamp, line, identity) = match event {
                                Event::Line(stamp, line) => {
                                    let identity = identity_record(&line);
                                    (stamp, line, identity)
                                }
                            };
                            let entry = append_event(
                                &directory,
                                stamp,
                                &line,
                                &mut rotated,
                                &mut sequence,
                                &mut budget,
                            )?;
                            // The identity journal is separately bounded and
                            // carries the small, sparse vocabulary the share
                            // exists to protect, so it takes the record even
                            // when the event segment's budget refused it.
                            if identity {
                                directory.append("identity.log", &entry, METADATA_LIMIT)?;
                            }
                        }
                        if synced.elapsed() >= Duration::from_secs(5)
                            || (empty && worker_stop.load(Ordering::Acquire))
                        {
                            for name in ["events.0.log", "identity.log"] {
                                match directory.sync(name) {
                                    Ok(()) => {}
                                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                                    Err(error) => return Err(error),
                                }
                            }
                            write_health(
                                &directory,
                                sequence,
                                discarded.load(Ordering::Relaxed),
                                rotated,
                                budget.total_suppressed(),
                                errors,
                                if worker_stop.load(Ordering::Acquire) {
                                    "stopped"
                                } else {
                                    "running"
                                },
                            )?;
                            synced = Instant::now();
                        }
                        Ok(())
                    })();
                    if result.is_err() {
                        errors = errors.saturating_add(1);
                        if !empty {
                            discarded.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    if empty && worker_stop.load(Ordering::Acquire) {
                        break;
                    }
                }
                let _ = finished_tx.send(());
            })?;
        Ok(Self { stop, finished })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // A wedged filesystem must not prevent TTY recovery. A stale health
        // record truthfully leaves the last unsynchronized tail unconfirmed.
        let _ = self.finished.recv_timeout(Duration::from_millis(500));
    }
}

fn write_health(
    directory: &Directory,
    sequence: u64,
    discarded: u64,
    rotated: u64,
    suppressed: u64,
    errors: u64,
    state: &str,
) -> io::Result<()> {
    directory.replace("health", &format!("sequence={sequence}\ndiscarded={discarded}\nrotated_bytes={rotated}\nsuppressed={suppressed}\nstorage_errors={errors}\nrecording={state}\nsynchronized_boot_msec={}\n", Stamp::now().boot_msec))
}

/// Append one record, rotating and enforcing the per-name share.
///
/// Formats the entry here rather than taking one, because rotation writes
/// records of its own: a segment that closes with names it had to suppress
/// says so at the head of the next one, and those records must carry the
/// sequence numbers *before* the record whose arrival rotated the segment.
/// The entry is reformatted after a rotation for exactly that reason, which
/// costs one `format!` per fifteen megabytes.
///
/// Returns the entry as written, for the identity journal to reuse.
fn append_event(
    directory: &Directory,
    stamp: Stamp,
    line: &str,
    rotated: &mut u64,
    sequence: &mut u64,
    budget: &mut SegmentBudget,
) -> io::Result<String> {
    use rustix::fs::OFlags;
    let format_entry = |sequence: u64| {
        format!(
            "{sequence}\t{}\t{}\t{line}\n",
            stamp.utc_msec, stamp.boot_msec
        )
    };
    let mut entry = format_entry(sequence.saturating_add(1));
    let file = directory.file(
        "events.0.log",
        OFlags::WRONLY | OFlags::CREATE | OFlags::APPEND,
    )?;
    if file.metadata()?.len() + entry.len() as u64 > SEGMENT_LIMIT {
        let oldest = format!("events.{}.log", SEGMENTS - 1);
        if let Ok(file) = directory.file(&oldest, OFlags::RDONLY) {
            *rotated = rotated.saturating_add(file.metadata()?.len());
        }
        for index in (0..SEGMENTS - 1).rev() {
            let source = directory.path.join(format!("events.{index}.log"));
            let target = directory.path.join(format!("events.{}.log", index + 1));
            match std::fs::rename(source, target) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        // What the closed segment refused, said out loud at the head of the
        // new one. A suppressed record is still evidence of its own kind's
        // volume, and silence here would make a bounded log read like a quiet
        // session.
        let suppressed = budget.rotate();
        if !suppressed.is_empty() {
            let mut file = directory.file(
                "events.0.log",
                OFlags::WRONLY | OFlags::CREATE | OFlags::APPEND,
            )?;
            for (name, count) in suppressed {
                *sequence = sequence.saturating_add(1);
                let notice = Stamp::now();
                file.write_all(
                    format!(
                        "{sequence}\t{}\t{}\tsophia_session_record_budget schema=1 status=suppressed name={name} suppressed={count} share_bytes={NAME_SEGMENT_SHARE}\n",
                        notice.utc_msec, notice.boot_msec,
                    )
                    .as_bytes(),
                )?;
            }
            entry = format_entry(sequence.saturating_add(1));
        }
    }
    *sequence = sequence.saturating_add(1);
    let name = budget_name(line);
    match budget.admit(name, entry.len() as u64) {
        Admission::Written => {}
        Admission::Suppressed => return Ok(entry),
        Admission::SuppressedFirst => {
            // Said where the cut begins, so the gap in a kind is visible in
            // the segment it happens in rather than inferable only from the
            // health total. The count follows at rotation.
            *sequence = sequence.saturating_add(1);
            let notice = Stamp::now();
            let mut file = directory.file(
                "events.0.log",
                OFlags::WRONLY | OFlags::CREATE | OFlags::APPEND,
            )?;
            file.write_all(
                format!(
                    "{sequence}\t{}\t{}\tsophia_session_record_budget schema=1 status=share_spent name={name} share_bytes={NAME_SEGMENT_SHARE}\n",
                    notice.utc_msec, notice.boot_msec,
                )
                .as_bytes(),
            )?;
            return Ok(entry);
        }
    }
    let mut file = directory.file(
        "events.0.log",
        OFlags::WRONLY | OFlags::CREATE | OFlags::APPEND,
    )?;
    file.write_all(entry.as_bytes())?;
    Ok(entry)
}

include!("capture/records.rs");
