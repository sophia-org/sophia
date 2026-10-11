//! The command that launches one X application into one confined client group.
//!
//! A COMMAND, NOT CUSTODY. What this builds is an unspawned
//! `std::process::Command`; it holds no child, waits for no peer and records
//! no `ProtectionDomainEvidence`. Whoever spawns it owns the child, its exit
//! and any claim about it. Supervised roles keep `spawn_bubblewrap`, which
//! does all three.
//!
//! The sandbox is the production Bubblewrap domain, under the exclusive
//! `ConfinedXApplication` role, with exactly two grants: the group's socket
//! directory read-only at the standard X11 path, and the session cookie file
//! read-only at a fixed path. Network, PID, IPC, UTS and cgroup namespaces,
//! `/dev`, `/proc`, `/tmp` and `/run` are the domain's own; no host home,
//! runtime directory, control or inspection socket is bound or named. Every
//! descriptor above stdio is marked close-on-exec in the child before
//! Bubblewrap runs, so nothing the spawner holds reaches the application.
//!
//! The checks below narrow replacement races between validation and
//! Bubblewrap's own pathname resolution; they do not pin the directory.

use super::protection::bubblewrap_command;
use super::{
    ProcessLaunchSpec, ProtectionDomainLaunchError, ProtectionDomainRole, ProtectionDomainSpec,
    ProtectionDomainSpecError, ProtectionPath,
};

use std::ffi::OsString;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the group's socket directory appears inside the sandbox.
pub const CONFINED_X_SOCKET_DIRECTORY: &str = "/tmp/.X11-unix";
/// Where the session cookie file appears inside the sandbox.
pub const CONFINED_X_AUTHORITY_FILE: &str = "/run/sophia-x/Xauthority";

/// One confined X launch, as the caller resolved it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfinedXApplicationLaunch {
    /// Absolute path of the application, as for every protected launch.
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    /// Absolute path of the Bubblewrap executable; never looked up on PATH.
    pub bubblewrap: PathBuf,
    /// The group directory, holding exactly the socket `X<display_number>`.
    pub group_directory: PathBuf,
    /// The session cookie file. Only this file is bound, never its directory.
    pub cookie_file: PathBuf,
    pub display_number: u32,
}

/// Why a confined launch was refused before any process existed.
#[derive(Debug)]
pub enum ConfinedXApplicationError {
    GroupDirectory {
        path: PathBuf,
        fault: ConfinedXPathFault,
    },
    CookieFile {
        path: PathBuf,
        fault: ConfinedXPathFault,
    },
    Domain(ProtectionDomainSpecError),
    Launch(ProtectionDomainLaunchError),
}

/// What was wrong with a path the launch would have bound.
#[derive(Debug)]
pub enum ConfinedXPathFault {
    Relative,
    Unreadable(std::io::Error),
    /// Refused, never followed.
    SymbolicLink,
    NotADirectory,
    NotARegularFile,
    /// Owned by another user.
    Foreign(u32),
    /// Group or other permission bits are set.
    Reachable(u32),
    /// The group directory holds an entry other than its socket.
    Occupied(OsString),
    /// The group directory does not hold its socket.
    MissingSocket,
    /// The entry named `X<n>` is not a socket.
    NotASocket,
}

impl core::fmt::Display for ConfinedXApplicationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::GroupDirectory { path, fault } => {
                write!(formatter, "group directory {}: {fault:?}", path.display())
            }
            Self::CookieFile { path, fault } => {
                write!(formatter, "cookie file {}: {fault:?}", path.display())
            }
            Self::Domain(error) => write!(formatter, "confined domain: {error}"),
            Self::Launch(error) => write!(formatter, "confined launch: {error}"),
        }
    }
}

impl std::error::Error for ConfinedXApplicationError {}

impl ConfinedXApplicationLaunch {
    /// Validate the grants and build the unspawned Bubblewrap command.
    ///
    /// Stdio is inherited. Bubblewrap and its version probe start with empty
    /// environments. The application gets `PATH`, `DISPLAY`, `XAUTHORITY`
    /// and Bubblewrap's `PWD=/` after `--clearenv`. The
    /// caller may redirect stdio or set a process group on the result, and
    /// must not add a `pre_exec` hook that hands a descriptor across.
    ///
    /// Bubblewrap runs with `--die-with-parent`, whose parent is the thread
    /// that spawns it: spawn from a thread that outlives the application.
    pub fn command(&self) -> Result<Command, ConfinedXApplicationError> {
        check_group_directory(&self.group_directory, self.display_number).map_err(|fault| {
            ConfinedXApplicationError::GroupDirectory {
                path: self.group_directory.clone(),
                fault,
            }
        })?;
        check_cookie_file(&self.cookie_file).map_err(|fault| {
            ConfinedXApplicationError::CookieFile {
                path: self.cookie_file.clone(),
                fault,
            }
        })?;
        let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::ConfinedXApplication])
            .and_then(|domain| {
                domain.path(ProtectionPath::read_only_at(
                    &self.group_directory,
                    CONFINED_X_SOCKET_DIRECTORY,
                ))
            })
            .and_then(|domain| {
                domain.path(ProtectionPath::read_only_at(
                    &self.cookie_file,
                    CONFINED_X_AUTHORITY_FILE,
                ))
            })
            .map_err(ConfinedXApplicationError::Domain)?
            .bubblewrap_path(&self.bubblewrap);
        let launch = self
            .args
            .iter()
            .fold(ProcessLaunchSpec::new(&self.executable), |launch, arg| {
                launch.arg(arg.clone())
            })
            .env("DISPLAY", format!(":{}", self.display_number))
            .env("XAUTHORITY", CONFINED_X_AUTHORITY_FILE);
        bubblewrap_command(&launch, &domain, confine_host_command)
            .map_err(ConfinedXApplicationError::Launch)
    }
}

fn confine_host_command(command: &mut Command) {
    // Apply to the version probe too: --clearenv only affects the sandbox,
    // after the host loader has already started Bubblewrap.
    command.env_clear();
    sophia_linux_peer::cloexec_inherited_fds_on_exec(command);
}

/// Owner-only, ours, a real directory, and holding exactly its socket: the
/// directory is mounted whole, so whatever else is in it is mounted too.
fn check_group_directory(path: &Path, display_number: u32) -> Result<(), ConfinedXPathFault> {
    check_owned(path, true)?;
    let socket = OsString::from(format!("X{display_number}"));
    let mut found = false;
    for entry in std::fs::read_dir(path).map_err(ConfinedXPathFault::Unreadable)? {
        let entry = entry.map_err(ConfinedXPathFault::Unreadable)?;
        if entry.file_name() != socket {
            return Err(ConfinedXPathFault::Occupied(entry.file_name()));
        }
        let metadata =
            std::fs::symlink_metadata(entry.path()).map_err(ConfinedXPathFault::Unreadable)?;
        if metadata.file_type().is_symlink() {
            return Err(ConfinedXPathFault::SymbolicLink);
        }
        if !metadata.file_type().is_socket() {
            return Err(ConfinedXPathFault::NotASocket);
        }
        found = true;
    }
    if !found {
        return Err(ConfinedXPathFault::MissingSocket);
    }
    Ok(())
}

fn check_cookie_file(path: &Path) -> Result<(), ConfinedXPathFault> {
    check_owned(path, false)
}

fn check_owned(path: &Path, directory: bool) -> Result<(), ConfinedXPathFault> {
    if !path.is_absolute() {
        return Err(ConfinedXPathFault::Relative);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(ConfinedXPathFault::Unreadable)?;
    let kind = metadata.file_type();
    if kind.is_symlink() {
        return Err(ConfinedXPathFault::SymbolicLink);
    }
    if directory && !kind.is_dir() {
        return Err(ConfinedXPathFault::NotADirectory);
    }
    if !directory && !kind.is_file() {
        return Err(ConfinedXPathFault::NotARegularFile);
    }
    let owner = metadata.uid();
    if owner != rustix::process::geteuid().as_raw() {
        return Err(ConfinedXPathFault::Foreign(owner));
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(ConfinedXPathFault::Reachable(mode));
    }
    Ok(())
}
