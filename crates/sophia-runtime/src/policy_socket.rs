use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::{ProtectionDomainEvidence, ProtectionDomainRole};

pub const SOPHIA_WM_SOCKET_ENV: &str = "SOPHIA_WM_SOCKET";
pub const SOPHIA_SHELL_SOCKET_ENV: &str = "SOPHIA_SHELL_SOCKET";
pub const SOPHIA_BROKER_SOCKET_ENV: &str = "SOPHIA_BROKER_SOCKET";
pub const SOPHIA_OUTPUT_SOCKET_ENV: &str = "SOPHIA_OUTPUT_SOCKET";

/// One exclusive policy role, and therefore one interface family.
///
/// A client's family is determined by the role socket it connects to and by the
/// message kind it sends; `ClientHello` deliberately carries no family field. Each
/// later authority takes its own role here rather than appearing as placeholder
/// messages inside an existing family. See `docs/sophia-policy-ipc.md`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyRole {
    /// `sophia_wm_v1`, the spatial-policy family.
    Wm,
    /// `sophia_shell_v1`, the metadata-bearing shell family.
    Shell,
    /// The metadata broker family.
    ///
    /// A separate role rather than a message inside another family, because the
    /// broker is a separate authority with its own disclosure budget: it publishes
    /// rules to authorities and descriptors to Engine, and neither of those is
    /// spatial policy. Sharing the WM socket would also give a broker peer the WM's
    /// admission, which is exactly the conflation the role split exists to prevent.
    Broker,
    /// `sophia_output_v1`, the exclusive physical-output authority. Session
    /// supervision may grant it to a WM or shell process without widening that
    /// process's WM/shell interface.
    Output,
}

impl PolicyRole {
    /// The socket file name beneath the session's private runtime directory.
    pub const fn socket_file_name(self) -> &'static str {
        match self {
            Self::Wm => "wm.sock",
            Self::Shell => "shell.sock",
            Self::Broker => "broker.sock",
            Self::Output => "output.sock",
        }
    }

    /// The environment variable advertising this role's socket path.
    pub const fn socket_env(self) -> &'static str {
        match self {
            Self::Wm => SOPHIA_WM_SOCKET_ENV,
            Self::Shell => SOPHIA_SHELL_SOCKET_ENV,
            Self::Broker => SOPHIA_BROKER_SOCKET_ENV,
            Self::Output => SOPHIA_OUTPUT_SOCKET_ENV,
        }
    }

    /// The protection-domain role a peer must carry to hold this policy role.
    ///
    /// Total rather than optional, so evidence is checked the same way for every
    /// role and a later role cannot be added without answering the question.
    pub const fn domain_role(self) -> ProtectionDomainRole {
        match self {
            Self::Wm => ProtectionDomainRole::SpatialPolicy,
            Self::Shell => ProtectionDomainRole::MetadataShell,
            Self::Broker => ProtectionDomainRole::MetadataBroker,
            Self::Output => ProtectionDomainRole::OutputAuthority,
        }
    }

    /// Whether a peer holding this role can observe application metadata.
    ///
    /// `docs/architecture.md` forbids blind spatial policy from sharing a
    /// protection domain with a metadata-bearing shell, broker, or application
    /// frontend. `ProtectionDomainSpec` refuses to build such a domain, but that
    /// check only fires for a caller that builds one: a caller that built none
    /// got no boundary and no complaint. These roles therefore refuse admission
    /// on a supervised PID alone and take `authorize_protected_peer` instead.
    ///
    /// The blind spatial-policy and output roles stay admissible without a
    /// domain. Requiring one everywhere is the separate decision about hosts
    /// with no `bwrap`, not a side effect of this rule.
    pub const fn is_metadata_bearing(self) -> bool {
        match self {
            Self::Wm | Self::Output => false,
            Self::Shell | Self::Broker => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyPeerIdentity {
    pub uid: u32,
    pub pid: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyRoleEndpointError {
    PathAlreadyExists,
    Io(String),
    UnauthorizedPeer {
        expected: PolicyPeerIdentity,
        actual: PolicyPeerIdentity,
    },
    PeerAlreadyActive,
    WrongReleasedPeer,
    PeerNotAuthorized,
    AcceptTimedOut,
    /// A metadata-bearing role was offered a peer with no protection domain.
    ProtectionDomainRequired {
        role: PolicyRole,
        required: ProtectionDomainRole,
    },
    /// A protection domain exists but does not carry this endpoint's role.
    ProtectionRoleMissing {
        required: ProtectionDomainRole,
        observed: BTreeSet<ProtectionDomainRole>,
    },
}

impl core::fmt::Display for PolicyRoleEndpointError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PolicyRoleEndpointError {}

/// Owner-only, session-hosted listener for one exclusive policy role.
pub struct PolicyRoleEndpoint {
    directory: PathBuf,
    socket_path: PathBuf,
    role: PolicyRole,
    listener: UnixListener,
    expected_uid: u32,
    expected_pid: Option<u32>,
    active_peer: Option<PolicyPeerIdentity>,
}

impl PolicyRoleEndpoint {
    pub fn bind(
        directory: impl AsRef<Path>,
        expected_peer: PolicyPeerIdentity,
    ) -> Result<Self, PolicyRoleEndpointError> {
        Self::bind_role(directory, PolicyRole::Wm, expected_peer)
    }

    /// Binds the owner-only listener for one role beneath a fresh mode-0700
    /// directory. The directory must not already exist, so two roles bind under
    /// separate parents rather than sharing one.
    ///
    /// A metadata-bearing role is refused here rather than at accept time.
    /// Naming an expected PID up front is admission on supervision alone, which
    /// is the boundary those roles may not cross; they bind through
    /// `bind_role_for_supervised_uid` and admit through
    /// `authorize_protected_peer`.
    pub fn bind_role(
        directory: impl AsRef<Path>,
        role: PolicyRole,
        expected_peer: PolicyPeerIdentity,
    ) -> Result<Self, PolicyRoleEndpointError> {
        if role.is_metadata_bearing() {
            return Err(PolicyRoleEndpointError::ProtectionDomainRequired {
                role,
                required: role.domain_role(),
            });
        }
        Self::bind_role_inner(directory, role, expected_peer.uid, Some(expected_peer.pid))
    }

    fn bind_role_inner(
        directory: impl AsRef<Path>,
        role: PolicyRole,
        expected_uid: u32,
        expected_pid: Option<u32>,
    ) -> Result<Self, PolicyRoleEndpointError> {
        let directory = directory.as_ref().to_path_buf();
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(PolicyRoleEndpointError::PathAlreadyExists);
            }
            Err(error) => {
                return Err(PolicyRoleEndpointError::Io(format!(
                    "create role directory: {error}"
                )));
            }
        }
        if let Err(error) = fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)) {
            let _ = fs::remove_dir(&directory);
            return Err(PolicyRoleEndpointError::Io(format!(
                "set role directory permissions: {error}"
            )));
        }
        let socket_path = directory.join(role.socket_file_name());
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = fs::remove_dir(&directory);
                return Err(PolicyRoleEndpointError::Io(format!(
                    "bind role socket: {error}"
                )));
            }
        };
        if let Err(error) = fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600)) {
            let _ = fs::remove_file(&socket_path);
            let _ = fs::remove_dir(&directory);
            return Err(PolicyRoleEndpointError::Io(format!(
                "set role socket permissions: {error}"
            )));
        }
        Ok(Self {
            directory,
            socket_path,
            role,
            listener,
            expected_uid,
            expected_pid,
            active_peer: None,
        })
    }

    pub fn bind_for_supervised_uid(
        directory: impl AsRef<Path>,
        expected_uid: u32,
    ) -> Result<Self, PolicyRoleEndpointError> {
        Self::bind_role_for_supervised_uid(directory, PolicyRole::Wm, expected_uid)
    }

    /// Binds a role whose peer is not known until session supervision spawns it.
    ///
    /// Open to every role, including the metadata-bearing ones: this constructor
    /// names no PID, so it makes no admission claim. The claim is made later, by
    /// whichever `authorize_*` call the role allows.
    pub fn bind_role_for_supervised_uid(
        directory: impl AsRef<Path>,
        role: PolicyRole,
        expected_uid: u32,
    ) -> Result<Self, PolicyRoleEndpointError> {
        Self::bind_role_inner(directory, role, expected_uid, None)
    }

    /// Admits a supervised peer by PID alone.
    ///
    /// Authentication, not isolation. A PID says which process connects, not what
    /// it can reach through ambient IPC, shared memory, inherited descriptors, or
    /// debugging. Metadata-bearing roles refuse this path.
    pub fn authorize_supervised_pid(&mut self, pid: u32) -> Result<(), PolicyRoleEndpointError> {
        if self.role.is_metadata_bearing() {
            return Err(PolicyRoleEndpointError::ProtectionDomainRequired {
                role: self.role,
                required: self.role.domain_role(),
            });
        }
        self.set_expected_pid(pid)
    }

    /// Admits the peer a protection domain actually launched.
    ///
    /// The PID and the domain roles are read from one launch record rather than
    /// correlated by the caller, so a supervisor that spawned the process
    /// unprotected has no evidence to offer and cannot reach this call.
    ///
    /// `ProtectionDomainEvidence` stays a passive record whose fields any caller
    /// can write, so this is a declaration the supervisor makes, not a proof the
    /// endpoint verifies. The boundary it closes is silent omission: building no
    /// domain used to admit anyway. Hand-writing evidence that contradicts the
    /// launch is a visible lie in the source instead.
    pub fn authorize_protected_peer(
        &mut self,
        evidence: &ProtectionDomainEvidence,
    ) -> Result<(), PolicyRoleEndpointError> {
        let required = self.role.domain_role();
        if !evidence.roles.contains(&required) {
            return Err(PolicyRoleEndpointError::ProtectionRoleMissing {
                required,
                observed: evidence.roles.clone(),
            });
        }
        self.set_expected_pid(evidence.peer_pid)
    }

    fn set_expected_pid(&mut self, pid: u32) -> Result<(), PolicyRoleEndpointError> {
        if pid == 0 || self.active_peer.is_some() {
            return Err(PolicyRoleEndpointError::PeerNotAuthorized);
        }
        self.expected_pid = Some(pid);
        Ok(())
    }

    pub fn accept_expected(&mut self) -> Result<UnixStream, PolicyRoleEndpointError> {
        let expected = self.expected_peer()?;
        let (stream, _) = self
            .listener
            .accept()
            .map_err(|error| PolicyRoleEndpointError::Io(error.to_string()))?;
        self.admit_expected_stream(stream, expected)
    }

    /// One nonblocking accept attempt with the same protected-peer check as
    /// blocking admission. Restores the listener mode before returning; no
    /// retry loop, sleep, or change to the admitted peer on WouldBlock/EINTR.
    pub fn poll_expected(&mut self) -> Result<Option<UnixStream>, PolicyRoleEndpointError> {
        let expected = self.expected_peer()?;
        self.listener
            .set_nonblocking(true)
            .map_err(|error| PolicyRoleEndpointError::Io(error.to_string()))?;
        let accepted = self.listener.accept();
        self.listener
            .set_nonblocking(false)
            .map_err(|error| PolicyRoleEndpointError::Io(error.to_string()))?;
        match accepted {
            Ok((stream, _)) => self.admit_expected_stream(stream, expected).map(Some),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(PolicyRoleEndpointError::Io(error.to_string())),
        }
    }

    pub fn accept_expected_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<UnixStream, PolicyRoleEndpointError> {
        let expected = self.expected_peer()?;
        self.listener
            .set_nonblocking(true)
            .map_err(|error| PolicyRoleEndpointError::Io(error.to_string()))?;
        let deadline = Instant::now() + timeout;
        let accepted = loop {
            match self.listener.accept() {
                Ok((stream, _)) => break Ok(stream),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    let now = Instant::now();
                    if now >= deadline {
                        break Err(PolicyRoleEndpointError::AcceptTimedOut);
                    }
                    std::thread::sleep(
                        deadline
                            .saturating_duration_since(now)
                            .min(Duration::from_millis(2)),
                    );
                }
                Err(error) => break Err(PolicyRoleEndpointError::Io(error.to_string())),
            }
        };
        self.listener
            .set_nonblocking(false)
            .map_err(|error| PolicyRoleEndpointError::Io(error.to_string()))?;
        self.admit_expected_stream(accepted?, expected)
    }

    fn expected_peer(&self) -> Result<PolicyPeerIdentity, PolicyRoleEndpointError> {
        if self.active_peer.is_some() {
            return Err(PolicyRoleEndpointError::PeerAlreadyActive);
        }
        let pid = self
            .expected_pid
            .ok_or(PolicyRoleEndpointError::PeerNotAuthorized)?;
        Ok(PolicyPeerIdentity {
            uid: self.expected_uid,
            pid,
        })
    }

    fn admit_expected_stream(
        &mut self,
        stream: UnixStream,
        expected: PolicyPeerIdentity,
    ) -> Result<UnixStream, PolicyRoleEndpointError> {
        let credentials = rustix::net::sockopt::socket_peercred(&stream)
            .map_err(|error| PolicyRoleEndpointError::Io(error.to_string()))?;
        let actual = PolicyPeerIdentity {
            uid: credentials.uid.as_raw(),
            pid: credentials.pid.as_raw_pid() as u32,
        };
        if actual != expected {
            return Err(PolicyRoleEndpointError::UnauthorizedPeer { expected, actual });
        }
        self.active_peer = Some(actual);
        Ok(stream)
    }

    pub fn release_peer(
        &mut self,
        peer: PolicyPeerIdentity,
    ) -> Result<(), PolicyRoleEndpointError> {
        if self.active_peer != Some(peer) {
            return Err(PolicyRoleEndpointError::WrongReleasedPeer);
        }
        self.active_peer = None;
        Ok(())
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub const fn role(&self) -> PolicyRole {
        self.role
    }

    pub const fn active_peer(&self) -> Option<PolicyPeerIdentity> {
        self.active_peer
    }
}

impl Drop for PolicyRoleEndpoint {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
        let _ = fs::remove_dir(&self.directory);
    }
}
