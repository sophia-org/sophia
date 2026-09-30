use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

pub struct Peer {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
    stderr: ChildStderr,
    pending: Vec<u8>,
    pub diagnostics: Vec<u8>,
    pub lines: VecDeque<String>,
    stdout_bytes: usize,
    pub stdout_closed: bool,
    reaped: bool,
}

impl Peer {
    pub fn spawn(binary: &Path, socket: &Path, mode: &str, fixture: &str) -> Result<Self, String> {
        // Inherit the qualification runner's private process group so its
        // outer deadline can terminate the owner and peer together.
        let mut child = Command::new(binary)
            .arg(socket)
            .arg(mode)
            .arg(fixture)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn C peer: {e}"))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut peer = Self {
            child,
            stdin: Some(stdin),
            stdout,
            stderr,
            pending: Vec::new(),
            diagnostics: Vec::new(),
            lines: VecDeque::new(),
            stdout_bytes: 0,
            stdout_closed: false,
            reaped: false,
        };
        peer.nonblocking()?;
        Ok(peer)
    }

    fn nonblocking(&mut self) -> Result<(), String> {
        for fd in [&self.stdout as &dyn std::os::fd::AsFd, &self.stderr] {
            let flags = fcntl_getfl(fd).map_err(|e| e.to_string())?;
            fcntl_setfl(fd, flags | OFlags::NONBLOCK).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn command(&mut self, byte: u8) -> Result<(), String> {
        self.stdin
            .as_mut()
            .ok_or("peer command channel closed")?
            .write_all(&[byte])
            .map_err(|e| format!("peer command: {e}"))
    }

    pub fn close_commands(&mut self) {
        self.stdin.take();
    }

    /// Drain both pipes on the owner thread. Caps include already consumed
    /// records, so a peer cannot evade the bound by producing small chunks.
    pub fn drain(&mut self) -> Result<(), String> {
        let mut bytes = [0u8; 4096];
        loop {
            match self.stdout.read(&mut bytes) {
                Ok(0) => {
                    self.stdout_closed = true;
                    if !self.pending.is_empty() {
                        return Err("truncated peer line".into());
                    }
                    break;
                }
                Ok(n) => {
                    self.stdout_bytes += n;
                    if self.stdout_bytes > 256 * 1024 {
                        return Err("peer stdout cap exceeded".into());
                    }
                    for &byte in &bytes[..n] {
                        if byte == b'\n' {
                            let line = String::from_utf8(std::mem::take(&mut self.pending))
                                .map_err(|_| "non-UTF8 peer line")?;
                            self.lines.push_back(line);
                        } else {
                            self.pending.push(byte);
                            if self.pending.len() > 192 {
                                return Err("peer line cap exceeded".into());
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("peer stdout: {e}")),
            }
        }
        loop {
            match self.stderr.read(&mut bytes) {
                Ok(0) => break,
                Ok(n) => {
                    if self.diagnostics.len() + n > 64 * 1024 {
                        return Err("peer stderr cap exceeded".into());
                    }
                    self.diagnostics.extend_from_slice(&bytes[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("peer stderr: {e}")),
            }
        }
        Ok(())
    }

    pub fn exited(&mut self) -> Result<Option<std::process::ExitStatus>, String> {
        let status = self.child.try_wait().map_err(|e| e.to_string())?;
        self.reaped |= status.is_some();
        Ok(status)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        if !self.reaped {
            // The child remains ours and unreaped; never signal a recycled PID.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
