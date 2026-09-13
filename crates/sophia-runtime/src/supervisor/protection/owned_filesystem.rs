use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _, symlink};
use std::path::{Component, Path, PathBuf};

const MAX_ENTRIES: usize = 64;
const MAX_FILE_BYTES: usize = 4096;
const MAX_TOTAL_FILE_BYTES: usize = 32 * 1024;

/// One entry in a bounded filesystem projected into a protected process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtectionFilesystemEntry {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

/// A normalized, bounded filesystem tree that the launcher materializes and
/// mounts read-only. Paths and symlink targets are relative to its root.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProtectionFilesystemManifest {
    entries: BTreeMap<PathBuf, ProtectionFilesystemEntry>,
}

impl ProtectionFilesystemManifest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn directory(&mut self, path: impl Into<PathBuf>) -> Result<(), String> {
        self.insert(path.into(), ProtectionFilesystemEntry::Directory)
    }

    pub fn file(&mut self, path: impl Into<PathBuf>, bytes: Vec<u8>) -> Result<(), String> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err("protected filesystem file exceeds 4096 bytes".into());
        }
        self.insert(path.into(), ProtectionFilesystemEntry::File(bytes))
    }

    pub fn symlink(
        &mut self,
        path: impl Into<PathBuf>,
        target: impl Into<PathBuf>,
    ) -> Result<(), String> {
        let path = path.into();
        let target = target.into();
        validate_relative_path(&target, true)?;
        validate_link_target(&path, &target)?;
        self.insert(path, ProtectionFilesystemEntry::Symlink(target))
    }

    pub fn entries(&self) -> &BTreeMap<PathBuf, ProtectionFilesystemEntry> {
        &self.entries
    }

    fn insert(&mut self, path: PathBuf, entry: ProtectionFilesystemEntry) -> Result<(), String> {
        validate_relative_path(&path, false)?;
        if self.entries.len() >= MAX_ENTRIES && !self.entries.contains_key(&path) {
            return Err("protected filesystem exceeds 64 entries".into());
        }
        if self.entries.contains_key(&path) {
            return Err("protected filesystem contains a duplicate path".into());
        }
        let added = match &entry {
            ProtectionFilesystemEntry::File(bytes) => bytes.len(),
            _ => 0,
        };
        let total = self
            .entries
            .values()
            .map(|entry| match entry {
                ProtectionFilesystemEntry::File(bytes) => bytes.len(),
                _ => 0,
            })
            .sum::<usize>()
            .saturating_add(added);
        if total > MAX_TOTAL_FILE_BYTES {
            return Err("protected filesystem exceeds 32768 file bytes".into());
        }
        self.entries.insert(path, entry);
        Ok(())
    }
}

fn validate_relative_path(path: &Path, allow_parent: bool) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err("protected filesystem paths must be nonempty and relative".into());
    }
    if path.components().any(|component| {
        matches!(component, Component::RootDir | Component::Prefix(_))
            || (!allow_parent && component == Component::ParentDir)
    }) {
        return Err("protected filesystem path is not normalized".into());
    }
    Ok(())
}

fn validate_link_target(path: &Path, target: &Path) -> Result<(), String> {
    resolved_link_target(path, target).map(|_| ())
}

fn resolved_link_target(path: &Path, target: &Path) -> Result<PathBuf, String> {
    let mut resolved = path
        .parent()
        .map_or_else(Vec::new, |parent| parent.components().collect::<Vec<_>>());
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => resolved.push(component),
            Component::ParentDir if !resolved.is_empty() => {
                resolved.pop();
            }
            Component::ParentDir => {
                return Err("protected filesystem symlink escapes its root".into());
            }
            Component::RootDir | Component::Prefix(_) => unreachable!(),
        }
    }
    Ok(resolved.iter().collect())
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct OwnedProtectionFilesystem {
    root: PathBuf,
    manifest: ProtectionFilesystemManifest,
}

impl OwnedProtectionFilesystem {
    pub(super) fn materialize(manifest: ProtectionFilesystemManifest) -> Result<Self, String> {
        validate_manifest(&manifest)?;
        let root = create_root()?;
        if let Err(error) = write_manifest(&root, &manifest) {
            thaw_directories(&root);
            let _ = fs::remove_dir_all(&root);
            return Err(error);
        }
        Ok(Self { root, manifest })
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for OwnedProtectionFilesystem {
    fn drop(&mut self) {
        thaw_directories(&self.root);
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn validate_manifest(manifest: &ProtectionFilesystemManifest) -> Result<(), String> {
    let directories = manifest
        .entries
        .iter()
        .filter_map(|(path, entry)| {
            matches!(entry, ProtectionFilesystemEntry::Directory).then_some(path.clone())
        })
        .collect::<BTreeSet<_>>();
    for (path, entry) in &manifest.entries {
        let mut parent = path.parent();
        while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
            if !directories.contains(path) {
                return Err(format!(
                    "protected filesystem omits parent directory {}",
                    path.display()
                ));
            }
            parent = path.parent();
        }
        if let ProtectionFilesystemEntry::Symlink(target) = entry {
            let resolved = resolved_link_target(path, target)?;
            if !matches!(
                manifest.entries.get(&resolved),
                Some(ProtectionFilesystemEntry::Directory | ProtectionFilesystemEntry::File(_))
            ) {
                return Err(format!(
                    "protected filesystem symlink {} has no concrete in-tree target",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn create_root() -> Result<PathBuf, String> {
    for _ in 0..16 {
        let mut nonce = [0_u8; 8];
        rustix::rand::getrandom(&mut nonce, rustix::rand::GetRandomFlags::empty())
            .map_err(|error| format!("protected filesystem random identity: {error}"))?;
        let suffix = u64::from_ne_bytes(nonce);
        let root = std::env::temp_dir().join(format!(
            "sophia-protection-{}-{suffix:016x}",
            std::process::id()
        ));
        match fs::create_dir(&root) {
            Ok(()) => {
                if let Err(error) = fs::set_permissions(&root, fs::Permissions::from_mode(0o700)) {
                    let _ = fs::remove_dir(&root);
                    return Err(format!("protected filesystem root mode: {error}"));
                }
                return Ok(root);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("protected filesystem root: {error}")),
        }
    }
    Err("protected filesystem could not allocate a unique root".into())
}

fn write_manifest(root: &Path, manifest: &ProtectionFilesystemManifest) -> Result<(), String> {
    for (path, entry) in &manifest.entries {
        let path = root.join(path);
        match entry {
            ProtectionFilesystemEntry::Directory => {
                fs::create_dir(&path)
                    .map_err(|error| format!("protected directory {}: {error}", path.display()))?;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|error| {
                    format!("protected directory mode {}: {error}", path.display())
                })?;
            }
            ProtectionFilesystemEntry::File(bytes) => {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path)
                    .map_err(|error| format!("protected file {}: {error}", path.display()))?;
                file.write_all(bytes)
                    .map_err(|error| format!("protected file write {}: {error}", path.display()))?;
                file.sync_all()
                    .map_err(|error| format!("protected file sync {}: {error}", path.display()))?;
                file.set_permissions(fs::Permissions::from_mode(0o400))
                    .map_err(|error| format!("protected file mode {}: {error}", path.display()))?;
            }
            ProtectionFilesystemEntry::Symlink(target) => symlink(target, &path)
                .map_err(|error| format!("protected symlink {}: {error}", path.display()))?,
        }
    }
    freeze_directories(root, &manifest.entries)
}

fn freeze_directories(
    root: &Path,
    entries: &BTreeMap<PathBuf, ProtectionFilesystemEntry>,
) -> Result<(), String> {
    let mut directories = entries
        .iter()
        .filter_map(|(path, entry)| {
            matches!(entry, ProtectionFilesystemEntry::Directory).then_some(path)
        })
        .collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for path in directories {
        fs::set_permissions(root.join(path), fs::Permissions::from_mode(0o500))
            .map_err(|error| format!("protected directory freeze {}: {error}", path.display()))?;
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o500))
        .map_err(|error| format!("protected filesystem freeze: {error}"))
}

fn thaw_directories(root: &Path) {
    let _ = fs::set_permissions(root, fs::Permissions::from_mode(0o700));
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                thaw_directories(&entry.path());
            }
        }
    }
}
