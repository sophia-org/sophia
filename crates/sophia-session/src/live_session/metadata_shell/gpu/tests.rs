#![cfg(test)]

use super::*;
use sophia_runtime::{ProtectionDomainRole, ProtectionDomainSpec};

fn identity(path: &Path) -> LiveRenderDeviceIdentitySnapshot {
    let metadata = std::fs::symlink_metadata(path).unwrap();
    LiveRenderDeviceIdentitySnapshot {
        node: path.to_path_buf(),
        device: metadata.dev(),
        inode: metadata.ino(),
        device_number: metadata.rdev(),
        physical_device: Path::new("/sys/devices/virtual/mem/null").to_path_buf(),
    }
}

fn base() -> ProcessLaunchSpec {
    ProcessLaunchSpec::new("/bin/true").protection_domain(
        ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell]).unwrap(),
    )
}

#[test]
fn denied_policy_carries_no_device_or_grant_environment() {
    let policy = ShellGpuLaunchPolicy::new(ShellGpuMode::Denied, None).unwrap();
    let (prepared, evidence) = policy.prepare(&base(), 7).unwrap();
    assert_eq!(evidence, None);
    assert!(prepared.environment.is_empty());
    assert!(prepared.protection_domain.unwrap().devices().is_empty());
}

#[test]
fn direct_policy_binds_exact_device_and_current_epoch() {
    let source = Path::new("/dev/null");
    let policy = ShellGpuLaunchPolicy::new(ShellGpuMode::Direct, Some(identity(source))).unwrap();
    let physical = policy.device.as_ref().unwrap().physical_device.clone();
    let (prepared, evidence) = policy
        .prepare_with_revalidation(&base(), 9, |_| Ok((1, 3, physical)))
        .unwrap();
    let environment = prepared
        .environment
        .iter()
        .map(|(key, value)| (key.to_str().unwrap(), value.to_str().unwrap()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(environment[GPU_MODE_ENV], "direct");
    assert_eq!(environment[GPU_GRANT_EPOCH_ENV], "9");
    assert_eq!(environment[GPU_RENDER_NODE_ENV], PRIVATE_RENDER_NODE);
    assert_eq!(environment[GPU_DEVICE_MAJOR_ENV], "1");
    assert_eq!(environment[GPU_DEVICE_MINOR_ENV], "3");
    let protection_domain = prepared.protection_domain.unwrap();
    let device = &protection_domain.devices()[0];
    assert_eq!(device.source, source);
    assert_eq!(device.destination, Path::new(PRIVATE_RENDER_NODE));
    assert_eq!(evidence.unwrap().epoch, 9);
}

#[test]
fn pci_identity_carries_vendor_and_device_fallbacks() {
    let root = std::env::temp_dir().join(format!("sophia-shell-pci-{}", std::process::id()));
    let directory = root.join("0000:03:00.0");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("vendor"), "0x1002\n").unwrap();
    std::fs::write(directory.join("device"), "0x744c\n").unwrap();
    assert_eq!(pci_bus_id(&directory).as_deref(), Some("0000:03:00.0"));
    assert_eq!(pci_ids(&directory).unwrap(), (0x1002, 0x744c));

    let source = Path::new("/dev/null");
    let policy = ShellGpuLaunchPolicy::new(ShellGpuMode::Direct, Some(identity(source))).unwrap();
    let (prepared, evidence) = policy
        .prepare_with_revalidation(&base(), 9, |_| Ok((1, 3, directory.clone())))
        .unwrap();
    let environment = prepared
        .environment
        .iter()
        .map(|(key, value)| (key.to_str().unwrap(), value.to_str().unwrap()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(environment[GPU_PCI_BUS_ID_ENV], "0000:03:00.0");
    assert_eq!(environment[GPU_PCI_VENDOR_ID_ENV], "1002");
    assert_eq!(environment[GPU_PCI_DEVICE_ID_ENV], "744c");
    assert_eq!(evidence.unwrap().pci_vendor_id, Some(0x1002));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn direct_policy_refuses_zero_epoch_and_changed_identity() {
    let source = Path::new("/dev/null");
    let mut changed = identity(source);
    changed.inode = changed.inode.saturating_add(1);
    let policy = ShellGpuLaunchPolicy::new(ShellGpuMode::Direct, Some(changed)).unwrap();
    assert!(policy.prepare(&base(), 7).is_err());

    let policy = ShellGpuLaunchPolicy::new(ShellGpuMode::Direct, Some(identity(source))).unwrap();
    assert!(policy.prepare(&base(), 0).is_err());
}

#[test]
fn policy_shape_must_match_the_declared_mode() {
    assert!(ShellGpuLaunchPolicy::new(ShellGpuMode::Direct, None).is_err());
    assert!(
        ShellGpuLaunchPolicy::new(ShellGpuMode::Denied, Some(identity(Path::new("/dev/null"))))
            .is_err()
    );
}

#[test]
fn only_direct_policy_observes_device_replacement() {
    let device = identity(Path::new("/dev/null"));
    let mut denied = ShellGpuLaunchPolicy::new(ShellGpuMode::Denied, None).unwrap();
    assert!(!denied.replace_device(Some(device.clone())));

    let mut direct = ShellGpuLaunchPolicy::new(ShellGpuMode::Direct, Some(device)).unwrap();
    assert!(!direct.replace_device(direct.device.clone()));
    assert!(direct.replace_device(None));
    assert!(!direct.replace_device(None));
}
