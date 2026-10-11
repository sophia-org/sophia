//! A fresh capability for one protected spawn, never part of a cloneable spec.

use std::{
    io,
    os::{fd::AsRawFd, unix::net::UnixStream},
    sync::Arc,
};

/// Names the inherited descriptor, not an authority claim. Session retains the
/// registration and generation; neither comes from this environment variable.
pub const LAUNCH_AUTH_FD_ENV: &str = "SOPHIA_LAUNCH_AUTH_FD";

#[derive(Debug)]
pub struct LaunchChannel {
    pub(super) generation: u64,
    pub(super) child: UnixStream,
    pub(super) reservation: LaunchReservation,
}

/// Parent-only identity of one socketpair reservation. Equal numeric launch
/// generations in different owners cannot stand in for this exact handoff.
#[derive(Clone, Debug)]
pub struct LaunchReservation {
    generation: u64,
    identity: Arc<()>,
}
impl LaunchReservation {
    pub fn generation(&self) -> u64 {
        self.generation
    }
}
impl PartialEq for LaunchReservation {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
    }
}
impl Eq for LaunchReservation {}

impl LaunchChannel {
    /// Session reserves the monotonic generation before calling this. The first
    /// endpoint stays with its relay; the second can be consumed by one start.
    /// Dropping a failed/unused handoff closes the child end. This provisions a
    /// channel only: it does not activate a launch or authenticate a principal.
    pub fn reserve(generation: u64) -> io::Result<(UnixStream, Self)> {
        if generation == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "zero launch generation",
            ));
        }
        let (owner, child) = UnixStream::pair()?;
        // A caller may have closed stdio. Neither end may occupy 0..=2: the
        // trusted relay end would otherwise leak as an inherited stdio stream.
        let owner = above_stdio(owner)?;
        let child = above_stdio(child)?;
        let reservation = LaunchReservation {
            generation,
            identity: Arc::new(()),
        };
        Ok((
            owner,
            Self {
                generation,
                child,
                reservation,
            },
        ))
    }

    pub fn reservation(&self) -> LaunchReservation {
        self.reservation.clone()
    }
}

fn above_stdio(stream: UnixStream) -> io::Result<UnixStream> {
    if stream.as_raw_fd() < 3 {
        Ok(UnixStream::from(rustix::io::fcntl_dupfd_cloexec(
            &stream, 3,
        )?))
    } else {
        Ok(stream)
    }
}
