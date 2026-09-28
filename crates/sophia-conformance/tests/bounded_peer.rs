//! Exercise the peer runner independently of the content protocol.
use std::{
    process::Command,
    time::{Duration, Instant},
};

#[path = "support/bounded_peer.rs"]
mod bounded_peer;

#[test]
fn drains_output_larger_than_a_pipe_without_deadlock() {
    let output = bounded_peer::run(
        Command::new("sh").args([
            "-c",
            "head -c 131072 /dev/zero; head -c 131072 /dev/zero >&2",
        ]),
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 131072);
    assert_eq!(output.stderr.len(), 131072);
}

#[test]
fn refuses_output_overflow_on_either_stream() {
    for (script, stream) in [
        ("head -c 1048577 /dev/zero", "stdout"),
        ("head -c 1048577 /dev/zero >&2", "stderr"),
    ] {
        let result = bounded_peer::run(
            Command::new("sh").args(["-c", script]),
            Duration::from_secs(5),
        );
        assert!(
            result
                .unwrap_err()
                .contains(&format!("{stream} exceeds cap"))
        );
    }
}

#[test]
fn an_inherited_pipe_cannot_outlive_the_deadline() {
    let start = Instant::now();
    let result = bounded_peer::run(
        Command::new("sh").args(["-c", "sleep 30 & exit 0"]),
        Duration::from_millis(150),
    );
    assert!(result.unwrap_err().contains("deadline"));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn successful_leader_cannot_leave_a_silent_group_member() {
    use rustix::process::{Pid, WaitOptions, waitpid};
    // Reap the orphan here instead of depending on the sandbox's PID 1. Poll
    // exactly this child; other tests own their own process groups.
    rustix::process::set_child_subreaper(Some(rustix::process::getpid())).unwrap();
    let output = bounded_peer::run(
        Command::new("sh").args(["-c", "sleep 30 >/dev/null 2>&1 & printf '%s\\n' \"$!\""]),
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(output.status.success());
    let pid = String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let child = Pid::from_raw(pid).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some((_, status)) = waitpid(Some(child), WaitOptions::NOHANG).unwrap() {
            assert_eq!(
                status.terminating_signal(),
                Some(rustix::process::Signal::KILL.as_raw())
            );
            break;
        }
        assert!(Instant::now() < deadline, "background child still running");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        std::fs::metadata(format!("/proc/{pid}"))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound
    );
}
