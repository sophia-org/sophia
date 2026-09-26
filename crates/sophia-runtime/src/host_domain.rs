use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;

const NAMESPACES: [&str; 3] = ["user", "mnt", "pid"];

pub(crate) struct HostDomain {
    uid: u32,
    namespaces: Vec<File>,
}

pub(crate) struct Peer {
    pub pid: u32,
    pidfd: OwnedFd,
    proc: File,
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "control host admission denied",
    )
}

impl HostDomain {
    pub fn new() -> io::Result<Self> {
        // Probe the kernel facility before publishing a discoverable endpoint.
        let (a, _b) = UnixStream::pair()?;
        let _ = sophia_linux_peer::socket_peer_pidfd(a.as_fd())?;
        Ok(Self {
            uid: rustix::process::geteuid().as_raw(),
            namespaces: NAMESPACES
                .iter()
                .map(|name| File::open(format!("/proc/self/ns/{name}")))
                .collect::<io::Result<_>>()?,
        })
    }

    pub fn admit(&self, stream: &UnixStream, excluded: &[u32]) -> io::Result<Peer> {
        let credentials = rustix::net::sockopt::socket_peercred(stream)?;
        if credentials.uid.as_raw() != self.uid {
            return Err(denied());
        }
        let pid = credentials.pid.as_raw_nonzero().get() as u32;
        let pidfd = sophia_linux_peer::socket_peer_pidfd(stream.as_fd())?;
        // An open proc directory does not retarget a recycled PID. Checking the
        // socket-derived pidfd on both sides of this open pins the association.
        alive(&pidfd)?;
        let proc = File::open(format!("/proc/{pid}"))?;
        let peer = Peer { pid, pidfd, proc };
        self.check(&peer, excluded)?;
        Ok(peer)
    }

    pub fn check(&self, peer: &Peer, excluded: &[u32]) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        alive(&peer.pidfd)?;
        if excluded.contains(&peer.pid) {
            return Err(denied());
        }
        let base = format!("/proc/self/fd/{}", peer.proc.as_raw_fd());
        for (name, expected) in NAMESPACES.iter().zip(&self.namespaces) {
            let observed = File::open(format!("{base}/ns/{name}"))?.metadata()?;
            let expected = expected.metadata()?;
            if (observed.dev(), observed.ino()) != (expected.dev(), expected.ino()) {
                return Err(denied());
            }
        }
        // Credentials can change after connect. No executable/path allowlist is
        // inferred here; reachable host-user processes are explicitly trusted.
        let mut status = String::new();
        File::open(format!("{base}/status"))?
            .take(65537)
            .read_to_string(&mut status)?;
        if status.len() > 65536 {
            return Err(denied());
        }
        let uid = status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .ok_or_else(denied)?;
        if uid.split_whitespace().count() != 4
            || !uid
                .split_whitespace()
                .all(|part| part.parse::<u32>().ok() == Some(self.uid))
        {
            return Err(denied());
        }
        alive(&peer.pidfd)
    }
}

fn alive(pidfd: &OwnedFd) -> io::Result<()> {
    let mut fds = [rustix::event::PollFd::new(
        pidfd,
        rustix::event::PollFlags::IN,
    )];
    if rustix::event::poll(
        &mut fds,
        Some(&rustix::event::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        }),
    )? != 0
    {
        return Err(denied());
    }
    Ok(())
}
