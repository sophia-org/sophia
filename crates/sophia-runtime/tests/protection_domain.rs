use sophia_runtime::{
    ProtectionDevice, ProtectionDeviceIdentity, ProtectionDomainRole, ProtectionDomainSpec,
    ProtectionDomainSpecError, ProtectionFilesystemManifest, ProtectionNetworkAccess,
    ProtectionPath,
};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

#[test]
fn wm_cannot_share_a_domain_with_metadata_roles() {
    for conflicting in [
        ProtectionDomainRole::MetadataShell,
        ProtectionDomainRole::MetadataBroker,
        ProtectionDomainRole::PortalBroker,
        ProtectionDomainRole::ApplicationFrontend,
    ] {
        assert_eq!(
            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::SpatialPolicy, conflicting,]),
            Err(ProtectionDomainSpecError::ForbiddenRoleComposition {
                spatial_policy: ProtectionDomainRole::SpatialPolicy,
                conflicting,
            })
        );
    }
}

#[test]
fn device_grants_are_character_devices_beneath_private_dev() {
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
        .unwrap()
        .device(ProtectionDevice::required_at(
            "/dev/null",
            "/dev/dri/renderD128",
        ))
        .unwrap();
    assert_eq!(domain.devices().len(), 1);
    assert_eq!(
        domain.devices()[0].source,
        std::path::Path::new("/dev/null")
    );

    assert_eq!(
        ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
            .unwrap()
            .device(ProtectionDevice::required_at("/dev/null", "/run/device")),
        Err(ProtectionDomainSpecError::InvalidDeviceDestination(
            "/run/device".into()
        ))
    );
    assert!(matches!(
        ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
            .unwrap()
            .device(ProtectionDevice::required_at(
                "/etc/ld.so.cache",
                "/dev/dri/renderD128"
            )),
        Err(ProtectionDomainSpecError::InvalidDeviceSource(_))
    ));
}

#[test]
fn device_grants_refuse_a_source_that_changed_after_policy_validation() {
    let previously_validated = std::fs::symlink_metadata("/dev/zero").unwrap();
    let expected = ProtectionDeviceIdentity::new(
        previously_validated.dev(),
        previously_validated.ino(),
        previously_validated.rdev(),
    );
    assert!(matches!(
        ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
            .unwrap()
            .device(ProtectionDevice::required_at_exact(
                "/dev/null",
                "/dev/dri/renderD128",
                expected,
            )),
        Err(ProtectionDomainSpecError::InvalidDeviceSource(path))
            if path == std::path::Path::new("/dev/null")
    ));
}

#[test]
fn bubblewrap_uses_device_bind_for_protection_devices() {
    const BACKEND: &str = include_str!("../src/supervisor/protection.rs");
    assert!(BACKEND.contains("for device in &domain.devices"));
    assert!(BACKEND.contains("\"--dev-bind\".into()"));
}

#[test]
fn owned_filesystem_is_bounded_frozen_and_retained_by_clones() {
    let mut manifest = ProtectionFilesystemManifest::new();
    manifest.directory("devices").unwrap();
    manifest.directory("devices/gpu").unwrap();
    manifest
        .file("devices/gpu/vendor", b"0x1002\n".to_vec())
        .unwrap();
    manifest.symlink("selected", "devices/gpu").unwrap();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
        .unwrap()
        .read_only_filesystem("/sys", manifest)
        .unwrap();
    let source = domain.paths()[0].source.clone();
    assert_eq!(domain.paths()[0].destination, std::path::Path::new("/sys"));
    assert_eq!(
        std::fs::metadata(&source).unwrap().permissions().mode() & 0o777,
        0o500
    );
    assert_eq!(
        std::fs::metadata(source.join("devices/gpu/vendor"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o400
    );
    assert_eq!(
        std::fs::read_link(source.join("selected")).unwrap(),
        std::path::Path::new("devices/gpu")
    );

    let retained = domain.clone();
    drop(domain);
    assert!(source.exists());
    drop(retained);
    assert!(!source.exists());
}

#[test]
fn owned_filesystem_refuses_duplicate_escape_and_oversized_content() {
    let mut duplicate = ProtectionFilesystemManifest::new();
    duplicate.directory("devices").unwrap();
    assert!(duplicate.directory("devices").is_err());

    let mut escape = ProtectionFilesystemManifest::new();
    assert!(escape.symlink("selected", "../host").is_err());

    let mut dangling = ProtectionFilesystemManifest::new();
    dangling.symlink("selected", "devices/gpu").unwrap();
    assert!(
        ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
            .unwrap()
            .read_only_filesystem("/sys", dangling)
            .is_err()
    );

    let mut oversized = ProtectionFilesystemManifest::new();
    assert!(oversized.file("blob", vec![0; 4097]).is_err());
}

#[test]
fn owned_filesystem_is_cleaned_when_later_domain_validation_fails() {
    let mut manifest = ProtectionFilesystemManifest::new();
    manifest.directory("devices").unwrap();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
        .unwrap()
        .read_only_filesystem("/sys", manifest)
        .unwrap();
    let source = domain.paths()[0].source.clone();
    assert!(source.exists());
    assert!(
        domain
            .path(ProtectionPath::read_only("/sys/devices"))
            .is_err()
    );
    assert!(!source.exists());
}

#[test]
fn path_grants_reject_root_aliases_and_overlapping_destinations() {
    assert_eq!(
        ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataBroker])
            .unwrap()
            .path(ProtectionPath::read_only("/tmp/../")),
        Err(ProtectionDomainSpecError::NonNormalizedPath(
            "/tmp/../".into()
        ))
    );

    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataBroker])
        .unwrap()
        .path(ProtectionPath::read_only("/run/sophia/metadata.sock"))
        .unwrap();
    assert_eq!(
        domain.path(ProtectionPath::read_write("/run/sophia")),
        Err(ProtectionDomainSpecError::OverlappingDestination {
            existing: "/run/sophia/metadata.sock".into(),
            requested: "/run/sophia".into(),
        })
    );
}

#[test]
fn wm_may_hold_the_output_role_in_its_own_domain() {
    let spec = ProtectionDomainSpec::bubblewrap([
        ProtectionDomainRole::SpatialPolicy,
        ProtectionDomainRole::OutputAuthority,
    ])
    .unwrap();
    assert!(spec.roles().contains(&ProtectionDomainRole::SpatialPolicy));
    assert!(
        spec.roles()
            .contains(&ProtectionDomainRole::OutputAuthority)
    );
    assert_eq!(spec.network(), ProtectionNetworkAccess::Denied);
}

/// The launcher reads the network policy rather than restating it.
///
/// `--unshare-net` used to sit as a literal in the argument prelude while
/// `ProtectionNetworkAccess` was stored, exposed by a getter, and never consulted.
/// With one variant the two agreed, but nothing made them agree: a second variant
/// would have been accepted by the builder and silently dropped at spawn.
///
/// That is the fail-open the Pnut audit found one step further along -- a network
/// policy whose configuration did not reach enforcement, where an explicitly
/// empty allowlist read as "unrestricted" instead of "deny all"
/// (`docs/pnut-evaluation.md`). Sophia's version was the same class with the
/// configuration reaching enforcement not at all.
///
/// Asserted against the source because the mapping is private to the bubblewrap
/// backend and belongs there: the policy is backend-neutral, and `--unshare-net`
/// is one backend's spelling of it. Making the builder public to observe it would
/// widen this crate's API for a test, and with a single variant a behavioural
/// assertion could only restate the mapping. What needs guarding is that the
/// mapping is consulted at all.
#[test]
fn the_launcher_derives_network_isolation_from_the_policy() {
    const BACKEND: &str = include_str!("../src/supervisor/protection.rs");

    let builder = BACKEND
        .find("fn bubblewrap_arguments(")
        .expect("the bubblewrap backend builds its own argument list");
    let prelude = &BACKEND[builder..];

    assert!(
        prelude.contains("args.extend(network_arguments(domain.network()));"),
        "the argument list must take its network flags from the policy"
    );
    assert!(
        !prelude.contains("\"--unshare-net\".into(),"),
        "no network flag may be restated as a literal beside the policy that \
         decides it"
    );

    // The mapping itself is exhaustive over the policy, so a new variant is a
    // compile error here rather than a flag that quietly stops being emitted.
    let mapping = BACKEND
        .find("fn network_arguments(")
        .expect("the backend maps a policy to its own flags");
    let mapping = &BACKEND[mapping..];
    assert!(
        mapping.contains("match network {"),
        "the mapping must be a match on the policy, not a conditional"
    );
    assert!(
        mapping.contains("ProtectionNetworkAccess::Denied => vec![\"--unshare-net\".into()]"),
        "the denied policy must still emit the flag it always emitted"
    );

    // And the value a caller can actually construct is still the denied one.
    let spec = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataBroker]).unwrap();
    assert_eq!(spec.network(), ProtectionNetworkAccess::Denied);
}
