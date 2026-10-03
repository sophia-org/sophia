//! The agent's side of a PAM attempt against real processes: the real
//! helper, and stand-ins that hang or exit without answering. A helper that
//! does not answer correctly and in time never yields an acceptance.

use sophia_factotum::pam_helper::PamHelper;
use sophia_factotum::pam_wire::HelperReply;
use sophia_factotum::proto::{PamRequest, PamVerdict};
use sophia_factotum::secret::SecretBytes;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, Instant};

/// Writing a script excludes every spawn: a fork taken while another
/// thread still holds a new script open for writing would keep that file
/// busy, and executing it would fail with ETXTBSY.
static FORKS: RwLock<()> = RwLock::new(());

fn verify(helper: &PamHelper, request: &PamRequest, cancelled: &AtomicBool) -> PamVerdict {
    let _forks = FORKS.read().unwrap_or_else(PoisonError::into_inner);
    helper.verify(request, cancelled)
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sophia-factotum-launch-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join(name);
        let _writing = FORKS.write().unwrap_or_else(PoisonError::into_inner);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn stack(&self, service: &str, stack: &str) -> &Path {
        std::fs::write(self.0.join(service), stack).unwrap();
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn request(service: &str) -> PamRequest {
    PamRequest {
        service: service.into(),
        user: "sophia-test-user".into(),
        secret: SecretBytes::from_slice(b"anything"),
    }
}

fn helper(path: PathBuf, confdir: Option<&Path>, deadline: Duration) -> PamHelper {
    PamHelper {
        path,
        confdir: confdir.map(Path::to_path_buf),
        deadline,
    }
}

#[test]
fn the_real_helper_gives_pams_verdict() {
    let scratch = Scratch::new("real");
    scratch.stack("deny", "auth required pam_deny.so\n");
    let confdir = scratch.stack("permit", "auth required pam_permit.so\n");
    let real = PathBuf::from(env!("CARGO_BIN_EXE_sophia-factotum-pam"));
    let helper = helper(real, Some(confdir), Duration::from_secs(10));
    let idle = AtomicBool::new(false);
    assert_eq!(
        verify(&helper, &request("permit"), &idle),
        PamVerdict::Accepted
    );
    assert_eq!(
        verify(&helper, &request("deny"), &idle),
        PamVerdict::Rejected
    );
}

#[test]
fn a_helper_past_its_deadline_is_killed_and_times_out() {
    let scratch = Scratch::new("hang");
    let hang = scratch.script("hang", "exec sleep 30");
    let started = Instant::now();
    let verdict = verify(
        &helper(hang, None, Duration::from_millis(300)),
        &request("any"),
        &AtomicBool::new(false),
    );
    assert_eq!(verdict, PamVerdict::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "killed, not waited for"
    );
}

#[test]
fn a_helper_that_exits_without_answering_fails() {
    let scratch = Scratch::new("silent");
    let silent = scratch.script("silent", "cat >/dev/null; exit 0");
    assert_eq!(
        verify(
            &helper(silent, None, Duration::from_secs(5)),
            &request("any"),
            &AtomicBool::new(false)
        ),
        PamVerdict::HelperFailed
    );
}

#[test]
fn a_forged_acceptance_from_a_helper_that_then_fails_is_not_taken() {
    let scratch = Scratch::new("forged");
    // A well-formed acceptance, then a failing exit.
    let forged = scratch.script(
        "forged",
        "cat >/dev/null; printf '\\014\\000\\000\\000SFPR\\001\\000\\000\\000'; exit 3",
    );
    assert_eq!(
        verify(
            &helper(forged, None, Duration::from_secs(5)),
            &request("any"),
            &AtomicBool::new(false)
        ),
        PamVerdict::HelperFailed
    );
}

#[test]
fn cancelling_stops_a_running_attempt_early() {
    let scratch = Scratch::new("cancel");
    let hang = scratch.script("hang", "exec sleep 30");
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancelled);
    let attempt = std::thread::spawn(move || {
        verify(
            &helper(hang, None, Duration::from_secs(20)),
            &request("any"),
            &flag,
        )
    });
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    cancelled.store(true, Ordering::Release);
    assert_eq!(attempt.join().unwrap(), PamVerdict::HelperFailed);
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// A shell `printf` argument writing `bytes` exactly.
fn octal(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("\\{byte:03o}")).collect()
}

fn accepted() -> [u8; 12] {
    HelperReply {
        verdict: PamVerdict::Accepted,
        pam_code: 0,
    }
    .encode()
}

/// The helper's process id, which the stand-in wrote first.
fn reaped(scratch: &Scratch) -> bool {
    let pid = std::fs::read_to_string(scratch.0.join("pid")).unwrap();
    !Path::new(&format!("/proc/{}", pid.trim())).exists()
}

#[test]
fn a_reply_split_across_writes_is_read_whole() {
    let scratch = Scratch::new("split");
    let reply = accepted();
    let split = scratch.script(
        "split",
        &format!(
            "printf '{}'; sleep 0.2; printf '{}'; exit 0",
            octal(&reply[..5]),
            octal(&reply[5..])
        ),
    );
    assert_eq!(
        verify(
            &helper(split, None, Duration::from_secs(5)),
            &request("any"),
            &AtomicBool::new(false)
        ),
        PamVerdict::Accepted
    );
}

#[test]
fn part_of_a_reply_and_then_a_stall_times_out_and_the_helper_is_reaped() {
    let scratch = Scratch::new("stall");
    let stall = scratch.script(
        "stall",
        &format!(
            "echo $$ > '{}/pid'; printf '{}'; exec sleep 30",
            scratch.0.display(),
            octal(&accepted()[..7])
        ),
    );
    let started = Instant::now();
    let verdict = verify(
        &helper(stall, None, Duration::from_millis(300)),
        &request("any"),
        &AtomicBool::new(false),
    );
    assert_eq!(verdict, PamVerdict::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "bounded by the deadline"
    );
    assert!(reaped(&scratch), "the stalled helper is killed and reaped");
}

#[test]
fn a_whole_acceptance_from_a_helper_that_never_exits_is_not_taken() {
    let scratch = Scratch::new("linger");
    let linger = scratch.script(
        "linger",
        &format!(
            "echo $$ > '{}/pid'; printf '{}'; exec sleep 30",
            scratch.0.display(),
            octal(&accepted())
        ),
    );
    let started = Instant::now();
    let verdict = verify(
        &helper(linger, None, Duration::from_millis(300)),
        &request("any"),
        &AtomicBool::new(false),
    );
    assert_eq!(verdict, PamVerdict::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the exit wait is bounded"
    );
    assert!(
        reaped(&scratch),
        "the lingering helper is killed and reaped"
    );
}

#[test]
fn part_of_a_reply_and_then_an_exit_fails() {
    let scratch = Scratch::new("short");
    let short = scratch.script(
        "short",
        &format!("printf '{}'; exit 0", octal(&accepted()[..11])),
    );
    assert_eq!(
        verify(
            &helper(short, None, Duration::from_secs(5)),
            &request("any"),
            &AtomicBool::new(false)
        ),
        PamVerdict::HelperFailed
    );
}

#[test]
fn cancelling_while_awaiting_the_exit_discards_the_reply() {
    let scratch = Scratch::new("cancel-exit");
    let linger = scratch.script(
        "linger",
        &format!(
            "echo $$ > '{}/pid'; printf '{}'; exec sleep 30",
            scratch.0.display(),
            octal(&accepted())
        ),
    );
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancelled);
    let attempt = std::thread::spawn(move || {
        verify(
            &helper(linger, None, Duration::from_secs(20)),
            &request("any"),
            &flag,
        )
    });
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    cancelled.store(true, Ordering::Release);
    assert_eq!(attempt.join().unwrap(), PamVerdict::HelperFailed);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(reaped(&scratch));
}
