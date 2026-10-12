//! Private fork/exec rendezvous. The parent records provenance before exec;
//! it supplies no identity or authentication verdict to the child.
use std::{
    io::{self, Read, Write},
    net::Shutdown,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    process::Command,
};

pub struct ExecGate {
    stream: UnixStream,
    bytes: [u8; 4],
    received: usize,
    released: bool,
}

#[derive(Debug)]
pub struct ExecGateChild {
    stream: UnixStream,
    wake: Option<OwnedFd>,
}

impl ExecGate {
    /// `wake`, when supplied, is a nonblocking eventfd owned by the caller's
    /// reactor. The hook writes one notification after publishing its PID.
    pub fn new(wake: Option<OwnedFd>) -> io::Result<(Self, ExecGateChild)> {
        if let Some(wake) = &wake {
            // Validate before fork: the notification must never block the
            // child, and cannot become an application-visible descriptor.
            // SAFETY: wake owns a live descriptor; fcntl only reads its flags.
            let (flags, descriptor) = unsafe {
                (
                    libc::fcntl(wake.as_raw_fd(), libc::F_GETFL),
                    libc::fcntl(wake.as_raw_fd(), libc::F_GETFD),
                )
            };
            if flags < 0 || descriptor < 0 {
                return Err(io::Error::last_os_error());
            }
            if flags & libc::O_NONBLOCK == 0 || descriptor & libc::FD_CLOEXEC == 0 {
                return Err(io::Error::from_raw_os_error(libc::EINVAL));
            }
        }
        let (owner, child) = UnixStream::pair()?;
        owner.set_nonblocking(true)?;
        Ok((
            Self {
                stream: owner,
                bytes: [0; 4],
                received: 0,
                released: false,
            },
            ExecGateChild {
                stream: child,
                wake,
            },
        ))
    }

    /// Nonblocking. This PID names the still-gated fork child in the parent's
    /// PID namespace. Capture its start time before releasing the gate.
    pub fn poll(&mut self) -> io::Result<Option<u32>> {
        while self.received < self.bytes.len() {
            match self.stream.read(&mut self.bytes[self.received..]) {
                Ok(0) => return Err(io::Error::from_raw_os_error(libc::ECANCELED)),
                Ok(count) => self.received += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        let pid = u32::from_ne_bytes(self.bytes);
        if pid == 0 || pid > i32::MAX as u32 {
            return Err(io::Error::from_raw_os_error(libc::EPROTO));
        }
        Ok(Some(pid))
    }

    /// Consumes the one-use gate. No release is permitted before PID capture.
    pub fn release(mut self) -> io::Result<()> {
        if self.received != self.bytes.len() {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        self.stream.write_all(&[1])?;
        self.released = true;
        Ok(())
    }
}
impl Drop for ExecGate {
    fn drop(&mut self) {
        if !self.released {
            // The fork child also inherited this endpoint before exec. Merely
            // closing the parent's copy cannot cancel it; shutdown affects all
            // copies and makes the child's read fail closed.
            let _ = self.stream.shutdown(Shutdown::Both);
        }
    }
}

impl ExecGateChild {
    /// Install after descriptor inheritance hooks. Both rendezvous descriptors
    /// are CLOEXEC; the hook never changes another descriptor's flags.
    pub fn install(self, command: &mut Command) {
        // SAFETY: the child executes only raw async-signal-safe syscalls using
        // captured descriptors and stack buffers. No allocation, locks, logging
        // or destructors run in the callback. Errors are raw errno values.
        unsafe {
            command.pre_exec(move || {
                let pid = (libc::getpid() as u32).to_ne_bytes();
                if libc::send(
                    self.stream.as_raw_fd(),
                    pid.as_ptr().cast(),
                    pid.len(),
                    libc::MSG_NOSIGNAL,
                ) != 4
                {
                    return Err(io::Error::from_raw_os_error(libc::EIO));
                }
                if let Some(wake) = &self.wake {
                    let value = 1u64.to_ne_bytes();
                    if libc::write(wake.as_raw_fd(), value.as_ptr().cast(), value.len()) != 8
                        && io::Error::last_os_error().raw_os_error() != Some(libc::EAGAIN)
                    {
                        return Err(io::Error::last_os_error());
                    }
                }
                let mut pending = libc::pollfd {
                    fd: self.stream.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                if libc::poll(&mut pending, 1, 10_000) <= 0 {
                    return Err(io::Error::from_raw_os_error(libc::ETIMEDOUT));
                }
                let mut approval = 0u8;
                if libc::recv(
                    self.stream.as_raw_fd(),
                    (&mut approval as *mut u8).cast(),
                    1,
                    0,
                ) != 1
                    || approval != 1
                {
                    return Err(io::Error::from_raw_os_error(libc::ECANCELED));
                }
                Ok(())
            });
        }
    }
}
