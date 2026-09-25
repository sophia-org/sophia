use super::super::sysfs::plan_at;
use super::super::*;
use sophia_runtime::{ProtectionDomainRole, ProtectionDomainSpec};
use std::os::unix::fs::symlink;
use std::time::{SystemTime, UNIX_EPOCH};

struct FakeSysfs {
    root: PathBuf,
    physical: PathBuf,
    target: PathBuf,
}

impl FakeSysfs {
    fn new(minor: u32) -> Self {
        let root = std::env::temp_dir().join(format!(
            "sophia-gpu-sysfs-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let physical =
            root.join("devices/pci0000:00/0000:00:01.1/0000:01:00.0/0000:02:00.0/0000:03:00.0");
        let target = physical.join(format!("drm/renderD{minor}"));
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(root.join("dev/char")).unwrap();
        std::fs::create_dir_all(root.join("class/drm")).unwrap();
        std::fs::create_dir_all(root.join("bus/pci")).unwrap();
        symlink(&target, root.join(format!("dev/char/226:{minor}"))).unwrap();
        symlink(&target, root.join(format!("class/drm/renderD{minor}"))).unwrap();
        symlink("../..", target.join("device")).unwrap();
        symlink(root.join("bus/pci"), physical.join("subsystem")).unwrap();
        std::fs::write(target.join("dev"), format!("226:{minor}\n")).unwrap();
        std::fs::write(
            target.join("uevent"),
            format!("MAJOR=226\nMINOR={minor}\nDEVNAME=dri/renderD{minor}\nDEVTYPE=drm_minor\n"),
        )
        .unwrap();
        std::fs::write(
            physical.join("uevent"),
            "DRIVER=amdgpu\nPCI_CLASS=30000\nPCI_ID=1002:744C\nPCI_SUBSYS_ID=1DA2:E471\nPCI_SLOT_NAME=0000:03:00.0\n",
        )
        .unwrap();
        for (name, value) in [
            ("vendor", "0x1002\n"),
            ("device", "0x744c\n"),
            ("subsystem_vendor", "0x1da2\n"),
            ("subsystem_device", "0xe471\n"),
            ("revision", "0xc8\n"),
        ] {
            std::fs::write(physical.join(name), value).unwrap();
        }
        Self {
            root,
            physical,
            target,
        }
    }

    fn identity(&self, minor: u32) -> LiveRenderDeviceIdentitySnapshot {
        LiveRenderDeviceIdentitySnapshot {
            node: PathBuf::from(format!("/dev/dri/renderD{minor}")),
            device: 0,
            inode: 0,
            device_number: 0,
            physical_device: self.physical.clone(),
        }
    }
}

impl Drop for FakeSysfs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn projection_contains_only_the_selected_pci_render_node_closure() {
    let fixture = FakeSysfs::new(129);
    std::fs::create_dir_all(fixture.physical.join("drm/card0")).unwrap();
    std::fs::write(fixture.physical.join("resource0"), "secret").unwrap();
    let unrelated = fixture
        .root
        .join("devices/pci0000:00/0000:00:02.0/0000:04:00.0/drm/renderD128");
    std::fs::create_dir_all(&unrelated).unwrap();
    symlink(&unrelated, fixture.root.join("dev/char/226:128")).unwrap();
    symlink(&unrelated, fixture.root.join("class/drm/renderD128")).unwrap();
    let planned = plan_at(
        &fixture.identity(129),
        226,
        129,
        &fixture.physical,
        &fixture.root,
    )
    .unwrap();
    assert_eq!(planned.render_name, "renderD129");
    assert_eq!(planned.pci_bus_id.as_deref(), Some("0000:03:00.0"));
    assert_eq!(planned.pci_ids, Some((0x1002, 0x744c)));
    let paths = planned
        .sysfs
        .entries()
        .keys()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let physical = "devices/pci0000:00/0000:00:01.1/0000:01:00.0/0000:02:00.0/0000:03:00.0";
    let render = format!("{physical}/drm/renderD129");
    let mut expected = vec![
        "bus".to_owned(),
        "bus/pci".to_owned(),
        "class".to_owned(),
        "class/drm".to_owned(),
        "class/drm/renderD129".to_owned(),
        "dev".to_owned(),
        "dev/char".to_owned(),
        "dev/char/226:129".to_owned(),
        "devices".to_owned(),
        "devices/pci0000:00".to_owned(),
        "devices/pci0000:00/0000:00:01.1".to_owned(),
        "devices/pci0000:00/0000:00:01.1/0000:01:00.0".to_owned(),
        "devices/pci0000:00/0000:00:01.1/0000:01:00.0/0000:02:00.0".to_owned(),
        physical.to_owned(),
        format!("{physical}/device"),
        format!("{physical}/drm"),
        format!("{physical}/revision"),
        format!("{physical}/subsystem"),
        format!("{physical}/subsystem_device"),
        format!("{physical}/subsystem_vendor"),
        format!("{physical}/uevent"),
        format!("{physical}/vendor"),
        render.clone(),
        format!("{render}/dev"),
        format!("{render}/device"),
        format!("{render}/subsystem"),
        format!("{render}/uevent"),
    ];
    expected.sort();
    assert_eq!(
        paths, expected,
        "the projection must have an exact allowlist"
    );

    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
        .unwrap()
        .read_only_filesystem("/sys", planned.sysfs)
        .unwrap();
    let projected = &domain.paths()[0].source;
    assert_eq!(
        std::fs::read_to_string(projected.join("dev/char/226:129/device/vendor")).unwrap(),
        "0x1002\n"
    );
    assert_eq!(
        std::fs::read_to_string(projected.join("dev/char/226:129/device/drm/renderD129/dev"))
            .unwrap(),
        "226:129\n"
    );
    assert!(
        std::fs::read_link(projected.join("dev/char/226:129/device/subsystem"))
            .unwrap()
            .ends_with("bus/pci")
    );
}

#[test]
fn projection_refuses_basename_topology_and_identity_disagreement() {
    let fixture = FakeSysfs::new(129);
    assert!(
        plan_at(
            &fixture.identity(128),
            226,
            129,
            &fixture.physical,
            &fixture.root,
        )
        .is_err()
    );

    std::fs::remove_file(fixture.target.join("dev")).unwrap();
    std::fs::write(fixture.target.join("dev"), "226:128\n").unwrap();
    assert!(
        plan_at(
            &fixture.identity(129),
            226,
            129,
            &fixture.physical,
            &fixture.root,
        )
        .is_err()
    );
}

#[test]
fn projection_refuses_duplicate_or_malformed_kernel_identity() {
    let fixture = FakeSysfs::new(128);
    std::fs::write(
        fixture.target.join("uevent"),
        "MAJOR=226\nMAJOR=226\nMINOR=128\nDEVNAME=dri/renderD128\n",
    )
    .unwrap();
    assert!(
        plan_at(
            &fixture.identity(128),
            226,
            128,
            &fixture.physical,
            &fixture.root,
        )
        .is_err()
    );
}
