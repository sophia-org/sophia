use sophia_linux_peer::ExecGate;
use std::{
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

struct Marker(PathBuf);
impl Marker {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "sophia-exec-gate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(!path.exists());
        Self(path)
    }
    fn command(&self) -> Command {
        let mut command = Command::new("/usr/bin/sh");
        command
            .args(["-c", "printf executed > \"$1\"", "exec-gate"])
            .arg(&self.0);
        command
    }
}
impl Drop for Marker {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn ready(gate: &mut ExecGate) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(pid) = gate.poll().unwrap() {
            return pid;
        }
        assert!(Instant::now() < deadline, "fork PID delivery deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn execution_waits_for_recorded_fork_identity_and_the_gate_is_single_use() {
    let marker = Marker::new();
    let (mut gate, child) = ExecGate::new(None).unwrap();
    let mut command = marker.command();
    child.install(&mut command);
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        let status = child.wait().unwrap();
        send.send((pid, status, command.spawn().is_err())).unwrap();
    });
    let pid = ready(&mut gate);
    assert!(!marker.0.exists(), "an unreleased child must not execute");
    assert!(matches!(receive.try_recv(), Err(mpsc::TryRecvError::Empty)));
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    assert!(
        stat.rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    gate.release().unwrap();
    let (actual, status, refused_reuse) = receive.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(
        actual, pid,
        "the captured identity must name the actual spawned child"
    );
    assert!(status.success());
    assert!(
        refused_reuse,
        "a released gate must not authorize another fork"
    );
    assert_eq!(std::fs::read(&marker.0).unwrap(), b"executed");
    worker.join().unwrap();
}

#[test]
fn cancellation_before_or_after_fork_prevents_execution() {
    for after_fork in [false, true] {
        let marker = Marker::new();
        let (mut gate, child) = ExecGate::new(None).unwrap();
        let mut command = marker.command();
        child.install(&mut command);
        let (send, receive) = mpsc::channel();
        let worker = if after_fork {
            let worker = std::thread::spawn(move || {
                send.send(command.spawn().is_err()).unwrap();
            });
            ready(&mut gate);
            drop(gate);
            worker
        } else {
            drop(gate);
            std::thread::spawn(move || {
                send.send(command.spawn().is_err()).unwrap();
            })
        };
        assert!(
            receive.recv_timeout(Duration::from_secs(3)).unwrap(),
            "cancelled child must fail spawn"
        );
        worker.join().unwrap();
        assert!(
            !marker.0.exists(),
            "cancelled handshake must prevent execution"
        );
    }
}
