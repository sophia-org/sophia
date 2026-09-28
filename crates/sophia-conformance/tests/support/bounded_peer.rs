//! Custody for trusted conformance peers. Drain both pipes while the peer runs;
//! bound output and time, then kill its process group before reaping its leader.
//! A peer that changes its process group or session is outside this helper's
//! cleanup scope. Like the session preparation checker, exit is observed with
//! WNOWAIT so the leader pins the group number through the final signal.
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions};
use std::{
    io::{ErrorKind, Read},
    os::{fd::AsFd, unix::process::CommandExt},
    process::{Child, ChildStderr, ChildStdout, Command, Output, Stdio},
    time::{Duration, Instant},
};

const OUTPUT_CAP: usize = 1024 * 1024;

pub struct Peer {
    child: Option<Child>,
    stdout: ChildStdout,
    stderr: ChildStderr,
    out: Vec<u8>,
    err: Vec<u8>,
    eof: (bool, bool),
    deadline: Instant,
}

impl Peer {
    pub fn spawn(command: &mut Command, timeout: Duration) -> Self {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .expect("spawn conformance peer");
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let peer = Self {
            child: Some(child),
            stdout,
            stderr,
            out: Vec::new(),
            err: Vec::new(),
            eof: (false, false),
            deadline: Instant::now() + timeout,
        };
        nonblocking(&peer.stdout);
        nonblocking(&peer.stderr);
        peer
    }

    pub fn id(&self) -> u32 {
        self.child.as_ref().expect("peer still owned").id()
    }

    /// One bounded visit; the caller can service its server between visits.
    pub fn poll(&mut self) -> Result<Option<Output>, String> {
        if !self.eof.0 {
            self.eof.0 = read(&mut self.stdout, &mut self.out, "stdout")?;
        }
        if !self.eof.1 {
            self.eof.1 = read(&mut self.stderr, &mut self.err, "stderr")?;
        }
        let pid = Pid::from_raw(self.id() as i32).unwrap();
        let exited = rustix::process::waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )
        .map_err(|error| format!("observe conformance peer: {error}"))?
        .is_some();
        if exited && self.eof == (true, true) {
            // The exited leader is still waitable, even if another group member
            // closed its pipes. No signal is sent after reaping this leader.
            let _ = rustix::process::kill_process_group(pid, Signal::KILL);
            let status = self
                .child
                .as_mut()
                .unwrap()
                .wait()
                .map_err(|error| format!("reap conformance peer: {error}"))?;
            self.child = None;
            return Ok(Some(Output {
                status,
                stdout: std::mem::take(&mut self.out),
                stderr: std::mem::take(&mut self.err),
            }));
        }
        if Instant::now() >= self.deadline {
            return Err(format!(
                "conformance peer deadline; stderr: {}",
                String::from_utf8_lossy(&self.err)
            ));
        }
        Ok(None)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let pid = Pid::from_raw(child.id() as i32).unwrap();
            let _ = rustix::process::kill_process_group(pid, Signal::KILL);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn nonblocking(pipe: &impl AsFd) {
    let flags = rustix::fs::fcntl_getfl(pipe).expect("read pipe flags");
    rustix::fs::fcntl_setfl(pipe, flags | rustix::fs::OFlags::NONBLOCK)
        .expect("make peer pipe nonblocking");
}

fn read(pipe: &mut impl Read, bytes: &mut Vec<u8>, name: &str) -> Result<bool, String> {
    let mut buffer = [0; 8192];
    let available = buffer.len().min(OUTPUT_CAP + 1 - bytes.len());
    match pipe.read(&mut buffer[..available]) {
        Ok(0) => Ok(true),
        Ok(count) => {
            bytes.extend_from_slice(&buffer[..count]);
            if bytes.len() > OUTPUT_CAP {
                return Err(format!("peer {name} exceeds cap"));
            }
            Ok(false)
        }
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
            Ok(false)
        }
        Err(error) => Err(format!("peer {name}: {error}")),
    }
}

pub fn run(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    let mut peer = Peer::spawn(command, timeout);
    loop {
        if let Some(output) = peer.poll()? {
            return Ok(output);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
