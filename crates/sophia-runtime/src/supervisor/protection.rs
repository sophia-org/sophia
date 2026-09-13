use super::*;

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

mod owned_filesystem;
use owned_filesystem::OwnedProtectionFilesystem;
pub use owned_filesystem::{ProtectionFilesystemEntry, ProtectionFilesystemManifest};

pub const DEFAULT_BUBBLEWRAP_PATH: &str = "/usr/bin/bwrap";
const BUBBLEWRAP_STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const MINIMUM_BUBBLEWRAP_VERSION: (u32, u32, u32) = (0, 11, 2);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProtectionDomainRole {
    SpatialPolicy,
    MetadataShell,
    MetadataBroker,
    PortalBroker,
    ApplicationFrontend,
    OutputAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtectionNetworkAccess {
    Denied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtectionPathAccess {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectionPath {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub access: ProtectionPathAccess,
    _owner: Option<Arc<OwnedProtectionFilesystem>>,
}

/// One required character device exposed at a fixed path in a private `/dev`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectionDevice {
    pub source: PathBuf,
    pub destination: PathBuf,
    identity: Option<ProtectionDeviceIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProtectionDeviceIdentity {
    filesystem_device: u64,
    inode: u64,
    device_number: u64,
}

impl ProtectionDevice {
    pub fn required_at(source: impl Into<PathBuf>, destination: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            identity: None,
        }
    }
}

impl ProtectionPath {
    pub fn read_only(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            source: path.clone(),
            destination: path,
            access: ProtectionPathAccess::ReadOnly,
            _owner: None,
        }
    }

    pub fn read_write(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            source: path.clone(),
            destination: path,
            access: ProtectionPathAccess::ReadWrite,
            _owner: None,
        }
    }

    pub fn read_write_at(source: impl Into<PathBuf>, destination: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            access: ProtectionPathAccess::ReadWrite,
            _owner: None,
        }
    }

    pub fn read_only_at(source: impl Into<PathBuf>, destination: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            access: ProtectionPathAccess::ReadOnly,
            _owner: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtectionDomainSpecError {
    NoRoles,
    ForbiddenRoleComposition {
        spatial_policy: ProtectionDomainRole,
        conflicting: ProtectionDomainRole,
    },
    RelativePath(PathBuf),
    NonNormalizedPath(PathBuf),
    RootBinding(PathBuf),
    OverlappingDestination {
        existing: PathBuf,
        requested: PathBuf,
    },
    InvalidDeviceSource(PathBuf),
    InvalidDeviceDestination(PathBuf),
    UnsupportedInheritedFd(i32),
    FilesystemMaterialization(String),
}

impl fmt::Display for ProtectionDomainSpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ProtectionDomainSpecError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectionDomainSpec {
    roles: BTreeSet<ProtectionDomainRole>,
    paths: Vec<ProtectionPath>,
    devices: Vec<ProtectionDevice>,
    inherited_fds: BTreeSet<i32>,
    network: ProtectionNetworkAccess,
    bubblewrap: PathBuf,
}

impl ProtectionDomainSpec {
    pub fn bubblewrap(
        roles: impl IntoIterator<Item = ProtectionDomainRole>,
    ) -> Result<Self, ProtectionDomainSpecError> {
        let roles = roles.into_iter().collect::<BTreeSet<_>>();
        validate_role_composition(&roles)?;
        Ok(Self {
            roles,
            paths: Vec::new(),
            devices: Vec::new(),
            inherited_fds: [0, 1, 2].into_iter().collect(),
            network: ProtectionNetworkAccess::Denied,
            bubblewrap: PathBuf::from(DEFAULT_BUBBLEWRAP_PATH),
        })
    }

    pub fn path(mut self, path: ProtectionPath) -> Result<Self, ProtectionDomainSpecError> {
        validate_binding_path(&path.source)?;
        validate_binding_path(&path.destination)?;
        if let Some(existing) = self
            .paths
            .iter()
            .find(|existing| paths_overlap(&existing.destination, &path.destination))
        {
            return Err(ProtectionDomainSpecError::OverlappingDestination {
                existing: existing.destination.clone(),
                requested: path.destination,
            });
        }
        self.paths.push(path);
        Ok(self)
    }

    /// Materialize and retain a bounded filesystem tree mounted read-only at
    /// `destination` for the lifetime of this protection-domain specification.
    pub fn read_only_filesystem(
        self,
        destination: impl Into<PathBuf>,
        manifest: ProtectionFilesystemManifest,
    ) -> Result<Self, ProtectionDomainSpecError> {
        let owner = Arc::new(
            OwnedProtectionFilesystem::materialize(manifest)
                .map_err(ProtectionDomainSpecError::FilesystemMaterialization)?,
        );
        let path = ProtectionPath {
            source: owner.root().to_path_buf(),
            destination: destination.into(),
            access: ProtectionPathAccess::ReadOnly,
            _owner: Some(owner),
        };
        self.path(path)
    }

    pub fn device(
        mut self,
        mut device: ProtectionDevice,
    ) -> Result<Self, ProtectionDomainSpecError> {
        validate_binding_path(&device.source)?;
        validate_binding_path(&device.destination)?;
        if !device.destination.starts_with("/dev/") || device.destination == Path::new("/dev") {
            return Err(ProtectionDomainSpecError::InvalidDeviceDestination(
                device.destination,
            ));
        }
        let metadata = std::fs::symlink_metadata(&device.source)
            .map_err(|_| ProtectionDomainSpecError::InvalidDeviceSource(device.source.clone()))?;
        if !metadata.file_type().is_char_device() {
            return Err(ProtectionDomainSpecError::InvalidDeviceSource(
                device.source,
            ));
        }
        device.identity = Some(ProtectionDeviceIdentity {
            filesystem_device: metadata.dev(),
            inode: metadata.ino(),
            device_number: metadata.rdev(),
        });
        if let Some(existing) = self
            .paths
            .iter()
            .map(|path| &path.destination)
            .chain(self.devices.iter().map(|device| &device.destination))
            .find(|existing| paths_overlap(existing, &device.destination))
        {
            return Err(ProtectionDomainSpecError::OverlappingDestination {
                existing: existing.clone(),
                requested: device.destination,
            });
        }
        self.devices.push(device);
        Ok(self)
    }

    pub fn inherited_fds(
        mut self,
        fds: impl IntoIterator<Item = i32>,
    ) -> Result<Self, ProtectionDomainSpecError> {
        let fds = fds.into_iter().collect::<BTreeSet<_>>();
        if let Some(fd) = fds.iter().find(|fd| !matches!(fd, 0..=2)) {
            return Err(ProtectionDomainSpecError::UnsupportedInheritedFd(*fd));
        }
        self.inherited_fds = fds;
        Ok(self)
    }

    pub fn bubblewrap_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.bubblewrap = path.into();
        self
    }

    pub fn roles(&self) -> &BTreeSet<ProtectionDomainRole> {
        &self.roles
    }

    pub fn paths(&self) -> &[ProtectionPath] {
        &self.paths
    }

    pub fn devices(&self) -> &[ProtectionDevice] {
        &self.devices
    }

    pub const fn network(&self) -> ProtectionNetworkAccess {
        self.network
    }

    pub fn bubblewrap_executable(&self) -> &Path {
        &self.bubblewrap
    }
}

fn validate_role_composition(
    roles: &BTreeSet<ProtectionDomainRole>,
) -> Result<(), ProtectionDomainSpecError> {
    if roles.is_empty() {
        return Err(ProtectionDomainSpecError::NoRoles);
    }
    if roles.contains(&ProtectionDomainRole::SpatialPolicy) {
        for conflicting in [
            ProtectionDomainRole::MetadataShell,
            ProtectionDomainRole::MetadataBroker,
            ProtectionDomainRole::PortalBroker,
            ProtectionDomainRole::ApplicationFrontend,
        ] {
            if roles.contains(&conflicting) {
                return Err(ProtectionDomainSpecError::ForbiddenRoleComposition {
                    spatial_policy: ProtectionDomainRole::SpatialPolicy,
                    conflicting,
                });
            }
        }
    }
    Ok(())
}

fn validate_binding_path(path: &Path) -> Result<(), ProtectionDomainSpecError> {
    if !path.is_absolute() {
        return Err(ProtectionDomainSpecError::RelativePath(path.to_path_buf()));
    }
    if path == Path::new("/") {
        return Err(ProtectionDomainSpecError::RootBinding(path.to_path_buf()));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(ProtectionDomainSpecError::NonNormalizedPath(
            path.to_path_buf(),
        ));
    }
    Ok(())
}

fn paths_overlap(first: &Path, second: &Path) -> bool {
    first.starts_with(second) || second.starts_with(first)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtectionBackendKind {
    Bubblewrap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectionDomainEvidence {
    pub backend: ProtectionBackendKind,
    pub supervisor_pid: u32,
    pub peer_pid: u32,
    pub roles: BTreeSet<ProtectionDomainRole>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtectionDomainLaunchError {
    RoleMismatch {
        process: SupervisedProcessKind,
        required: ProtectionDomainRole,
    },
    InvalidExecutable(PathBuf),
    InvalidBinding(PathBuf),
    Spawn(String),
    StartupTimedOut,
    InvalidStatus(String),
    UnsupportedVersion(String),
}

impl fmt::Display for ProtectionDomainLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ProtectionDomainLaunchError {}

pub(crate) struct ProtectedProcess {
    pub child: Child,
    pub evidence: ProtectionDomainEvidence,
}

pub(crate) fn spawn_bubblewrap(
    process: SupervisedProcessKind,
    launch: &ProcessLaunchSpec,
    domain: &ProtectionDomainSpec,
) -> Result<ProtectedProcess, ProtectionDomainLaunchError> {
    let required_role = required_role(process);
    if !domain.roles.contains(&required_role) {
        return Err(ProtectionDomainLaunchError::RoleMismatch {
            process,
            required: required_role,
        });
    }
    let program = PathBuf::from(&launch.program);
    if !program.is_absolute() || !program.is_file() {
        return Err(ProtectionDomainLaunchError::InvalidExecutable(program));
    }
    for binding in &domain.paths {
        if !binding.source.exists() {
            return Err(ProtectionDomainLaunchError::InvalidBinding(
                binding.source.clone(),
            ));
        }
    }
    for device in &domain.devices {
        let current = std::fs::symlink_metadata(&device.source).ok();
        let unchanged = current.as_ref().is_some_and(|metadata| {
            metadata.file_type().is_char_device()
                && device.identity.as_ref().is_some_and(|identity| {
                    metadata.dev() == identity.filesystem_device
                        && metadata.ino() == identity.inode
                        && metadata.rdev() == identity.device_number
                })
        });
        if !unchanged {
            return Err(ProtectionDomainLaunchError::InvalidBinding(
                device.source.clone(),
            ));
        }
    }
    let observed_version = bubblewrap_version(&domain.bubblewrap)?;
    let parsed_version = parse_bubblewrap_version(&observed_version)
        .ok_or_else(|| ProtectionDomainLaunchError::UnsupportedVersion(observed_version.clone()))?;
    if parsed_version < MINIMUM_BUBBLEWRAP_VERSION {
        return Err(ProtectionDomainLaunchError::UnsupportedVersion(
            observed_version,
        ));
    }

    let args = bubblewrap_arguments(launch, domain, &program)?;
    let mut command = Command::new(&domain.bubblewrap);
    command.args(args);
    if launch.process_group {
        command.process_group(0);
    }
    if !domain.inherited_fds.contains(&0) {
        command.stdin(Stdio::null());
    }
    if !domain.inherited_fds.contains(&1) {
        command.stdout(Stdio::null());
    }
    if !domain.inherited_fds.contains(&2) {
        command.stderr(Stdio::null());
    }
    let mut child = command
        .spawn()
        .map_err(|error| ProtectionDomainLaunchError::Spawn(error.to_string()))?;
    let supervisor_pid = child.id();
    let peer_pid = match wait_for_bubblewrap_peer(&mut child, supervisor_pid) {
        Ok(pid) => pid,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    Ok(ProtectedProcess {
        child,
        evidence: ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid,
            peer_pid,
            roles: domain.roles.clone(),
        },
    })
}

fn required_role(process: SupervisedProcessKind) -> ProtectionDomainRole {
    match process {
        SupervisedProcessKind::WindowManager => ProtectionDomainRole::SpatialPolicy,
        SupervisedProcessKind::PortalBroker => ProtectionDomainRole::PortalBroker,
        SupervisedProcessKind::MetadataBroker => ProtectionDomainRole::MetadataBroker,
        SupervisedProcessKind::Shell => ProtectionDomainRole::MetadataShell,
        SupervisedProcessKind::SophiaXAuthority => ProtectionDomainRole::ApplicationFrontend,
    }
}

/// The bubblewrap flags one network policy requires.
///
/// Kept in this backend rather than on `ProtectionNetworkAccess`, because the
/// policy is backend-neutral by design and `--unshare-net` is a bubblewrap
/// spelling of it. A Landlock-based backend would satisfy the same `Denied` with
/// a handled-access mask instead, and `docs/pnut-evaluation.md` names that as the
/// intended second occupant of this seam.
fn network_arguments(network: ProtectionNetworkAccess) -> Vec<OsString> {
    match network {
        ProtectionNetworkAccess::Denied => vec!["--unshare-net".into()],
    }
}

fn bubblewrap_arguments(
    launch: &ProcessLaunchSpec,
    domain: &ProtectionDomainSpec,
    program: &Path,
) -> Result<Vec<OsString>, ProtectionDomainLaunchError> {
    let mut args: Vec<OsString> = vec![
        "--unshare-user".into(),
        "--unshare-ipc".into(),
        "--unshare-pid".into(),
    ];
    // In the slot the literal `--unshare-net` used to occupy, so the emitted
    // command line is byte-identical and this change is provably inert. The
    // flag and the field agreed only because the enum has one variant, and
    // nothing made them: a second variant would have been accepted by the
    // builder and dropped here, which is the shape of the fail-open the Pnut
    // audit found -- a network policy whose configuration never reached
    // enforcement. This is the one isolation claim the spec lets a caller
    // state, so it is the one that has to be read back.
    args.extend(network_arguments(domain.network()));
    args.extend([
        "--unshare-uts".into(),
        "--unshare-cgroup".into(),
        "--disable-userns".into(),
        "--assert-userns-disabled".into(),
        "--new-session".into(),
        "--die-with-parent".into(),
        "--as-pid-1".into(),
        "--cap-drop".into(),
        "ALL".into(),
        "--hostname".into(),
        "sophia-domain".into(),
        "--clearenv".into(),
        "--ro-bind".into(),
        "/usr".into(),
        "/usr".into(),
        "--symlink".into(),
        "usr/bin".into(),
        "/bin".into(),
        "--symlink".into(),
        "usr/lib".into(),
        "/lib".into(),
        "--symlink".into(),
        "usr/lib".into(),
        "/lib64".into(),
        "--dir".into(),
        "/etc".into(),
        "--ro-bind".into(),
        "/etc/ld.so.cache".into(),
        "/etc/ld.so.cache".into(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        "--dir".into(),
        "/run".into(),
        "--dir".into(),
        "/home".into(),
    ]);

    let mut created = BTreeSet::new();
    created.extend([
        PathBuf::from("/etc"),
        PathBuf::from("/run"),
        PathBuf::from("/home"),
    ]);
    if !program.starts_with("/usr/") {
        append_parent_directories(&mut args, &mut created, program, true)?;
        args.extend(["--ro-bind".into(), program.into(), program.into()]);
    }
    for binding in &domain.paths {
        append_parent_directories(
            &mut args,
            &mut created,
            &binding.destination,
            binding.source.is_file(),
        )?;
        let option = match binding.access {
            ProtectionPathAccess::ReadOnly => "--ro-bind",
            ProtectionPathAccess::ReadWrite => "--bind",
        };
        args.extend([
            option.into(),
            binding.source.as_os_str().to_owned(),
            binding.destination.as_os_str().to_owned(),
        ]);
    }
    for device in &domain.devices {
        append_parent_directories(&mut args, &mut created, &device.destination, true)?;
        args.extend([
            "--dev-bind".into(),
            device.source.as_os_str().to_owned(),
            device.destination.as_os_str().to_owned(),
        ]);
    }
    for (key, value) in &launch.environment {
        args.extend(["--setenv".into(), key.clone(), value.clone()]);
    }
    args.extend([
        "--setenv".into(),
        "PATH".into(),
        "/usr/bin".into(),
        "--chdir".into(),
        "/".into(),
        "--".into(),
        program.as_os_str().to_owned(),
    ]);
    args.extend(launch.args.iter().cloned());
    Ok(args)
}

fn append_parent_directories(
    args: &mut Vec<OsString>,
    created: &mut BTreeSet<PathBuf>,
    destination: &Path,
    destination_is_file: bool,
) -> Result<(), ProtectionDomainLaunchError> {
    if !destination.is_absolute() || destination == Path::new("/") {
        return Err(ProtectionDomainLaunchError::InvalidBinding(
            destination.to_path_buf(),
        ));
    }
    let parent = if destination_is_file {
        destination.parent()
    } else {
        Some(destination)
    };
    let mut parents = parent
        .into_iter()
        .flat_map(Path::ancestors)
        .filter(|path| *path != Path::new("/"))
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    parents.reverse();
    for parent in parents {
        if created.insert(parent.clone()) {
            args.extend(["--dir".into(), parent.into_os_string()]);
        }
    }
    Ok(())
}

fn wait_for_bubblewrap_peer(
    child: &mut Child,
    supervisor_pid: u32,
) -> Result<u32, ProtectionDomainLaunchError> {
    let children = PathBuf::from(format!(
        "/proc/{supervisor_pid}/task/{supervisor_pid}/children"
    ));
    let deadline = std::time::Instant::now() + BUBBLEWRAP_STARTUP_TIMEOUT;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| ProtectionDomainLaunchError::Spawn(error.to_string()))?
        {
            return Err(ProtectionDomainLaunchError::InvalidStatus(format!(
                "bubblewrap exited before its role peer was ready: {status}"
            )));
        }
        if let Ok(value) = std::fs::read_to_string(&children)
            && let Some(pid) = parse_single_child_pid(&value)
        {
            return Ok(pid);
        }
        if std::time::Instant::now() >= deadline {
            return Err(ProtectionDomainLaunchError::StartupTimedOut);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn parse_single_child_pid(value: &str) -> Option<u32> {
    let mut children = value.split_ascii_whitespace();
    let pid = children.next()?.parse::<u32>().ok()?;
    if pid == 0 || children.next().is_some() {
        return None;
    }
    Some(pid)
}

pub fn bubblewrap_version(path: impl AsRef<OsStr>) -> Result<String, ProtectionDomainLaunchError> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|error| ProtectionDomainLaunchError::Spawn(error.to_string()))?;
    if !output.status.success() {
        return Err(ProtectionDomainLaunchError::Spawn(
            "bubblewrap --version failed".to_owned(),
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|error| ProtectionDomainLaunchError::InvalidStatus(error.to_string()))
}

fn parse_bubblewrap_version(value: &str) -> Option<(u32, u32, u32)> {
    let version = value.strip_prefix("bubblewrap ")?.trim();
    let mut components = version.split('.');
    let major = components.next()?.parse().ok()?;
    let minor = components.next()?.parse().ok()?;
    let patch = components.next()?.parse().ok()?;
    if components.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}
