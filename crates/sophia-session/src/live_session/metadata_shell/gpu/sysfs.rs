use sophia_backend_live::LiveRenderDeviceIdentitySnapshot;
use sophia_runtime::ProtectionFilesystemManifest;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

const MAX_SYSFS_LEAF_BYTES: usize = 4096;

pub(super) struct RevalidatedGpuDevice {
    pub major: u32,
    pub minor: u32,
    pub render_name: String,
    pub pci_bus_id: Option<String>,
    pub pci_ids: Option<(u32, u32)>,
    pub sysfs: ProtectionFilesystemManifest,
}

pub(super) fn plan(
    identity: &LiveRenderDeviceIdentitySnapshot,
    major: u32,
    minor: u32,
    physical: &Path,
) -> Result<RevalidatedGpuDevice, String> {
    plan_at(identity, major, minor, physical, Path::new("/sys"))
}

pub(super) fn plan_at(
    identity: &LiveRenderDeviceIdentitySnapshot,
    major: u32,
    minor: u32,
    physical: &Path,
    sys_root: &Path,
) -> Result<RevalidatedGpuDevice, String> {
    let render_name = identity
        .node
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("shell render node has no UTF-8 kernel basename")?
        .to_owned();
    if render_name != format!("renderD{minor}") {
        return Err("shell render node basename disagrees with its kernel minor".into());
    }

    let dev_char = sys_root.join(format!("dev/char/{major}:{minor}"));
    let class_node = sys_root.join("class/drm").join(&render_name);
    let target = std::fs::canonicalize(&dev_char)
        .map_err(|error| format!("shell render node sysfs identity: {error}"))?;
    let class_target = std::fs::canonicalize(&class_node)
        .map_err(|error| format!("shell render node class identity: {error}"))?;
    if target != class_target {
        return Err("shell render node sysfs identities disagree".into());
    }
    let observed_physical = std::fs::canonicalize(target.join("device"))
        .map_err(|error| format!("shell render node physical identity: {error}"))?;
    if observed_physical != physical || observed_physical != identity.physical_device {
        return Err("shell render node physical device changed during discovery".into());
    }
    let pci_subsystem = std::fs::canonicalize(observed_physical.join("subsystem"))
        .map_err(|error| format!("shell render node bus identity: {error}"))?;
    if pci_subsystem != sys_root.join("bus/pci") {
        return Err("shell render node is not a supported PCI DRM device".into());
    }
    if target != observed_physical.join("drm").join(&render_name) {
        return Err("shell render node is not the selected physical device's DRM child".into());
    }

    let dev = read_leaf(&target.join("dev"))?;
    if text(&dev)?.trim() != format!("{major}:{minor}") {
        return Err("shell render node sysfs dev identity disagrees".into());
    }
    let node_uevent = read_leaf(&target.join("uevent"))?;
    let node_values = parse_uevent(&node_uevent)?;
    require_value(&node_values, "MAJOR", &major.to_string())?;
    require_value(&node_values, "MINOR", &minor.to_string())?;
    require_value(&node_values, "DEVNAME", &format!("dri/{render_name}"))?;

    let physical_uevent = read_leaf(&observed_physical.join("uevent"))?;
    let physical_values = parse_uevent(&physical_uevent)?;
    let pci_bus_id = normalized_pci_bus_id(
        physical_values
            .get("PCI_SLOT_NAME")
            .ok_or("shell render node PCI uevent omits PCI_SLOT_NAME")?,
    )?;
    if observed_physical.file_name().and_then(|name| name.to_str()) != Some(&pci_bus_id) {
        return Err("shell render node PCI path disagrees with PCI_SLOT_NAME".into());
    }

    let vendor = read_leaf(&observed_physical.join("vendor"))?;
    let device = read_leaf(&observed_physical.join("device"))?;
    let subsystem_vendor = read_leaf(&observed_physical.join("subsystem_vendor"))?;
    let subsystem_device = read_leaf(&observed_physical.join("subsystem_device"))?;
    let vendor_id = parse_hex_leaf("vendor", &vendor, u16::MAX.into())?;
    let device_id = parse_hex_leaf("device", &device, u16::MAX.into())?;
    parse_hex_leaf("subsystem_vendor", &subsystem_vendor, u16::MAX.into())?;
    parse_hex_leaf("subsystem_device", &subsystem_device, u16::MAX.into())?;
    let revision = match read_optional_leaf(&observed_physical.join("revision"))? {
        Some(bytes) => {
            parse_hex_leaf("revision", &bytes, u8::MAX.into())?;
            Some(bytes)
        }
        None => None,
    };

    let target_relative = relative_to_sysfs(&target, sys_root)?;
    let physical_relative = relative_to_sysfs(&observed_physical, sys_root)?;
    let mut directories = BTreeSet::new();
    for path in [
        Path::new("dev/char"),
        Path::new("class/drm"),
        Path::new("bus/pci"),
        target_relative.as_path(),
        physical_relative.as_path(),
    ] {
        add_directories(&mut directories, path);
    }
    let mut manifest = ProtectionFilesystemManifest::new();
    let mut directories = directories.into_iter().collect::<Vec<_>>();
    directories.sort_by_key(|path| path.components().count());
    for directory in directories {
        manifest.directory(directory)?;
    }

    let dev_char_relative = PathBuf::from(format!("dev/char/{major}:{minor}"));
    let class_relative = Path::new("class/drm").join(&render_name);
    manifest.symlink(
        &dev_char_relative,
        relative_link(&dev_char_relative, &target_relative)?,
    )?;
    manifest.symlink(
        &class_relative,
        relative_link(&class_relative, &target_relative)?,
    )?;
    manifest.file(target_relative.join("dev"), dev)?;
    manifest.file(target_relative.join("uevent"), node_uevent)?;
    manifest.symlink(
        target_relative.join("device"),
        relative_link(&target_relative.join("device"), &physical_relative)?,
    )?;
    manifest.symlink(
        target_relative.join("subsystem"),
        relative_link(&target_relative.join("subsystem"), Path::new("class/drm"))?,
    )?;
    manifest.file(physical_relative.join("uevent"), physical_uevent)?;
    manifest.file(physical_relative.join("vendor"), vendor)?;
    manifest.file(physical_relative.join("device"), device)?;
    manifest.file(physical_relative.join("subsystem_vendor"), subsystem_vendor)?;
    manifest.file(physical_relative.join("subsystem_device"), subsystem_device)?;
    if let Some(revision) = revision {
        manifest.file(physical_relative.join("revision"), revision)?;
    }
    manifest.symlink(
        physical_relative.join("subsystem"),
        relative_link(&physical_relative.join("subsystem"), Path::new("bus/pci"))?,
    )?;

    if std::fs::canonicalize(&dev_char).ok() != Some(target.clone())
        || std::fs::canonicalize(&class_node).ok() != Some(target)
        || std::fs::canonicalize(observed_physical.join("subsystem")).ok()
            != Some(sys_root.join("bus/pci"))
    {
        return Err("shell render node sysfs topology changed during discovery".into());
    }

    Ok(RevalidatedGpuDevice {
        major,
        minor,
        render_name,
        pci_bus_id: Some(pci_bus_id),
        pci_ids: Some((vendor_id, device_id)),
        sysfs: manifest,
    })
}

fn relative_to_sysfs(path: &Path, sys_root: &Path) -> Result<PathBuf, String> {
    path.strip_prefix(sys_root)
        .map(Path::to_path_buf)
        .map_err(|_| "shell GPU sysfs identity escaped the sysfs root".into())
}

fn add_directories(directories: &mut BTreeSet<PathBuf>, path: &Path) {
    for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        directories.insert(ancestor.to_path_buf());
    }
}

fn relative_link(link: &Path, target: &Path) -> Result<PathBuf, String> {
    let from = link.parent().unwrap_or(Path::new(""));
    let from_parts = from.components().collect::<Vec<_>>();
    let target_parts = target.components().collect::<Vec<_>>();
    if from_parts
        .iter()
        .any(|part| !matches!(part, Component::Normal(_)))
        || target_parts
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("shell GPU sysfs link endpoints are not normalized".into());
    }
    let shared = from_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut result = PathBuf::new();
    for _ in shared..from_parts.len() {
        result.push("..");
    }
    for component in &target_parts[shared..] {
        result.push(component.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    Ok(result)
}

fn read_leaf(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("shell GPU sysfs leaf {}: {error}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_SYSFS_LEAF_BYTES as u64 {
        return Err(format!(
            "shell GPU sysfs leaf {} is not a bounded regular file",
            path.display()
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("shell GPU sysfs leaf {}: {error}", path.display()))?;
    if bytes.len() > MAX_SYSFS_LEAF_BYTES {
        return Err(format!(
            "shell GPU sysfs leaf {} exceeds 4096 bytes",
            path.display()
        ));
    }
    Ok(bytes)
}

fn read_optional_leaf(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match read_leaf(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(_) if !path.exists() => Ok(None),
        Err(error) => Err(error),
    }
}

fn text(bytes: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(bytes).map_err(|_| "shell GPU sysfs leaf is not UTF-8".into())
}

fn parse_uevent(bytes: &[u8]) -> Result<BTreeMap<String, String>, String> {
    let mut values = BTreeMap::new();
    for line in text(bytes)?.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or("shell GPU sysfs uevent line is malformed")?;
        if key.is_empty() || value.is_empty() || values.insert(key.into(), value.into()).is_some() {
            return Err("shell GPU sysfs uevent contains an invalid or duplicate key".into());
        }
    }
    Ok(values)
}

fn require_value(
    values: &BTreeMap<String, String>,
    key: &str,
    expected: &str,
) -> Result<(), String> {
    if values.get(key).map(String::as_str) == Some(expected) {
        Ok(())
    } else {
        Err(format!("shell GPU sysfs uevent {key} disagrees"))
    }
}

fn normalized_pci_bus_id(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    if bytes.len() != 12
        || bytes[4] != b':'
        || bytes[7] != b':'
        || bytes[10] != b'.'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7 | 10) || byte.is_ascii_hexdigit())
    {
        return Err("shell render node PCI bus identity is malformed".into());
    }
    let function = value[11..]
        .parse::<u8>()
        .map_err(|_| "shell render node PCI function is malformed")?;
    if function > 7 {
        return Err("shell render node PCI function exceeds seven".into());
    }
    Ok(value.to_ascii_lowercase())
}

fn parse_hex_leaf(name: &str, bytes: &[u8], maximum: u32) -> Result<u32, String> {
    let value = text(bytes)?.trim();
    let value = value.strip_prefix("0x").unwrap_or(value);
    let value = u32::from_str_radix(value, 16)
        .map_err(|_| format!("shell render node PCI {name} is malformed"))?;
    (value <= maximum)
        .then_some(value)
        .ok_or_else(|| format!("shell render node PCI {name} exceeds its bound"))
}
