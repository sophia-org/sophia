//! The agent process: hardening, the helper's integrity, and the loop that
//! serves Session's connection until Session hangs up.

use crate::export::FactotumExport;
use crate::jobs::JobPool;
use crate::pam_helper::PamHelper;
use crate::proto::{ChannelClass, Settings};
use sophia_9p::records::Limits;
use sophia_9p::unix::Server;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// How the agent hardened itself; logged by name only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Hardening {
    pub undumpable: bool,
    pub no_core: bool,
    /// Whether future allocations are locked as well as current pages.
    pub locked_future: bool,
    pub locked_current: bool,
}

/// Makes the agent undumpable (no ptrace or `/proc` reads by same-UID
/// processes), gives it no core, and locks its memory. Future pages are
/// locked only when the memlock limit covers `budget`; under a smaller limit
/// a later allocation would fail and abort the agent, and with it every
/// unlock until a restart.
pub fn harden(budget: u64) -> Hardening {
    use rustix::mm::{MlockAllFlags, mlockall};
    use rustix::process::{
        DumpableBehavior, Resource, Rlimit, getrlimit, set_dumpable_behavior, setrlimit,
    };

    let undumpable = set_dumpable_behavior(DumpableBehavior::NotDumpable).is_ok();
    let no_core = setrlimit(
        Resource::Core,
        Rlimit {
            current: Some(0),
            maximum: Some(0),
        },
    )
    .is_ok();
    let roomy = getrlimit(Resource::Memlock)
        .current
        .is_none_or(|limit| limit >= budget);
    let locked_future = roomy && mlockall(MlockAllFlags::CURRENT | MlockAllFlags::FUTURE).is_ok();
    let locked_current = locked_future || mlockall(MlockAllFlags::CURRENT).is_ok();
    Hardening {
        undumpable,
        no_core,
        locked_future,
        locked_current,
    }
}

/// The helper is executed with the user's password, so a helper the user
/// could replace would be a password oracle of the user's choosing. The
/// path is resolved through every symlink, and the file and each directory
/// above it up to `/` must belong to root and be writable by no one else.
/// The agent then executes the returned canonical path: no component of it
/// can be renamed or replaced by the user between this check and an exec.
pub fn check_helper(path: &Path) -> Result<PathBuf, &'static str> {
    if !path.is_absolute() {
        return Err("pam helper path is not absolute");
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| "pam helper is missing")?;
    let metadata = std::fs::metadata(&canonical).map_err(|_| "pam helper is missing")?;
    if !metadata.is_file() || metadata.mode() & 0o111 == 0 {
        return Err("pam helper is not an executable file");
    }
    if !protected(&metadata) {
        return Err("pam helper is not root-owned and protected");
    }
    for ancestor in canonical.ancestors().skip(1) {
        let directory =
            std::fs::metadata(ancestor).map_err(|_| "pam helper directory is missing")?;
        if !protected(&directory) {
            return Err("a pam helper directory is not root-owned and protected");
        }
    }
    Ok(canonical)
}

/// Owned by root and writable by neither group nor others.
fn protected(metadata: &std::fs::Metadata) -> bool {
    metadata.uid() == 0 && metadata.mode() & 0o022 == 0
}

/// Whether the peer of Session's socket is this process's parent, running as
/// the same user.
pub fn session_is_parent(session: &UnixStream) -> bool {
    let Ok(peer) = rustix::net::sockopt::socket_peercred(session) else {
        return false;
    };
    peer.uid == rustix::process::geteuid() && rustix::process::getppid() == Some(peer.pid)
}

pub struct AgentConfig {
    pub settings: Settings,
    pub helper: PamHelper,
    pub workers: usize,
}

/// Serves Session's connection until it closes. The agent serves no other
/// connection: the user endpoint opens only when t275 admits one.
pub fn serve(config: AgentConfig, session: UnixStream) -> std::io::Result<()> {
    // A 4095-byte rpc write and its header always fit; nothing larger is
    // ever needed.
    let limits = Limits::new(16_384, 8_192, 32, 64, 32_768, 8)
        .map_err(|error| std::io::Error::other(format!("factotum limits: {error:?}")))?;
    let mut server = Server::new(FactotumExport::new(config.settings), limits)?;
    let wake = server.wake();
    let (pool, outcomes) = JobPool::start(config.workers, config.helper, move || wake.wake());
    server.export_mut().attach_jobs(Box::new(pool), outcomes);
    let connection = server.adopt(session).map_err(|refused| refused.error)?;
    server.export_mut().admit(connection, ChannelClass::Session);
    while server.connection_count() != 0 {
        server.turn(None)?;
    }
    Ok(())
}
