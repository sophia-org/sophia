//! Reduced grants reach the SDK unchanged. Two full-size sealed images remain
//! resident within a 4/8/4 MiB staging/resident/retiring profile. Source FDs cross
//! the real image endpoint; authentication verdicts are supplied.
#[allow(dead_code)] // Shared file-component fixture; this file drives only part of it.
#[path = "support/file_component.rs"]
mod file_component;

use file_component::*;
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::ContentLifecycle;

fn grant(epoch: u64) -> ContentGrant {
    ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    }
}

/// The floor profile: S = M, R = 2M, T = M, with M and every other field at
/// the prototype value.
fn floor(epoch: u64) -> ContentLimits {
    let mut limits = ContentLimits::prototype(grant(epoch));
    limits.max_staging_bytes = 4 * MIB;
    limits.max_resident_bytes = 8 * MIB;
    limits.max_retiring_bytes = 4 * MIB;
    limits
}

#[test]
fn floor_limits_reach_the_file_client_as_its_first_welcome_record() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut component = Component::new();
    let limits = floor(1);
    component
        .transport
        .reserve_content(&mut registry, limits.clone())
        .unwrap();
    // The registry charges exactly the reduced profile, not the prototype.
    assert_eq!(registry.reserved_bytes(), 16 * MIB);
    assert_eq!(registry.reserved_backing_bytes(), 12 * MIB);
    let (welcome, client) = component.negotiate(&mut registry, 1, CONTENT);
    welcome.unwrap();
    let (client, received) = client.unwrap();
    assert_eq!(received, ShellContentRecord::Limits(limits.clone()));
    let ShellContentRecord::Limits(received) = received else {
        unreachable!()
    };
    // What a client does with the first record: validate it and build its
    // lifecycle from it; both accept the reduced profile.
    received.validate().unwrap();
    assert_eq!(received.max_resource_bytes, 4 * MIB);
    assert_eq!(
        received.max_session_retiring_bytes,
        ContentLimits::prototype(received.grant).max_session_retiring_bytes
    );
    ContentLifecycle::new(received).unwrap();
    drop(client);
    component.disconnect(&mut registry);
    registry.collect();
    assert!(registry.accounting().quiescent());
}

#[test]
fn floor_limits_hold_two_full_size_sealed_images_resident() {
    full_size_images_with_limits(floor(1));
}

#[test]
fn reduced_chunk_limits_hold_two_full_size_sealed_images_resident() {
    let mut limits = floor(1);
    limits.max_chunk_bytes = 32768;
    limits.validate().unwrap();
    // 1024-pixel rows: eight fit this grant, rather than the prototype's
    // fifteen. Both the SDK and the real export must use the granted layout.
    assert_eq!(limits.max_chunk_bytes / 4096, 8);
    assert_eq!(
        ContentLimits::prototype(limits.grant).max_chunk_bytes / 4096,
        15
    );
    full_size_images_with_limits(limits);
}

fn full_size_images_with_limits(limits: ContentLimits) {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut component = Component::new();
    component
        .transport
        .reserve_content(&mut registry, limits.clone())
        .unwrap();
    component.connect(&mut registry, limits.clone());
    let first = component.upload_pixels(
        &mut registry,
        limits.grant,
        1,
        1024,
        1024,
        &[30, 0, 0, 255].repeat(1024 * 1024),
        0,
    );
    let second = component.upload_pixels(
        &mut registry,
        limits.grant,
        2,
        1024,
        1024,
        &[90, 0, 0, 255].repeat(1024 * 1024),
        0,
    );
    assert_eq!(first.cpu_bytes().unwrap().len() as u64, 4 * MIB);
    assert!(
        first
            .cpu_bytes()
            .unwrap()
            .chunks_exact(4)
            .all(|p| p == [30, 0, 0, 255])
    );
    assert!(
        second
            .cpu_bytes()
            .unwrap()
            .chunks_exact(4)
            .all(|p| p == [90, 0, 0, 255])
    );
    let accounting = registry.accounting();
    assert_eq!(accounting.memory.resident, 8 * MIB);
    assert_eq!(accounting.memory.staging, 0);
    // A third full-size resource cannot fit: the reduced resident bound is
    // exactly two, which is what the floor promises and no more.
    assert!(accounting.memory.resident + 4 * MIB > limits.max_resident_bytes);
    drop((first, second));
    component.disconnect(&mut registry);
    registry.collect();
    assert!(registry.accounting().quiescent());
}

/// Incoherent profiles must be refused before either wire reserves an owner.
#[test]
fn incoherent_reduced_profiles_are_refused_before_any_file_reservation() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut component = Component::new();
    let mut cases = Vec::new();
    // Each byte class below the unchanged per-resource bound.
    for class in 0..3 {
        let mut limits = floor(1);
        match class {
            0 => limits.max_staging_bytes = 4 * MIB - 4,
            1 => limits.max_resident_bytes = 4 * MIB - 4,
            _ => limits.max_retiring_bytes = 4 * MIB - 4,
        }
        cases.push((limits, ContentStoreError::Malformed));
    }
    // A coherent profile whose session bound is not the registry's own.
    let mut session = floor(1);
    session.max_session_retiring_bytes = 16 * MIB;
    cases.push((session, ContentStoreError::Budget));
    for (limits, expected) in cases {
        assert!(matches!(
            component.transport.reserve_content(&mut registry, limits),
            Err(ShellTransportError::ContentStore(error)) if error == expected
        ));
        assert_eq!(registry.reserved_bytes(), 0);
        assert_eq!(registry.accounting().active_epochs, 0);
    }
    // No refusal published a watermark: the same epoch still admits.
    component
        .transport
        .reserve_content(&mut registry, floor(1))
        .unwrap();
    assert_eq!(registry.reserved_bytes(), 16 * MIB);
}

#[test]
fn a_client_refuses_an_incoherent_limits_object() {
    let header = ShellFileHeader {
        kind: ShellFileKind::Limits,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 0,
    };
    let valid = encode_shell_file_limits(header, floor(1)).unwrap();
    assert_eq!(decode_shell_file_limits(&valid).unwrap(), floor(1));
    // The Limits body follows the 32-byte file header and is 264 bytes:
    // resource bound at 24, staging 32, resident 40, retiring 48, session 56.
    assert_eq!(valid.len(), 32 + 264);
    let body = 32;
    // Control: each offset holds the field it is named for, so the refusals
    // below come from that field's value.
    let limits = floor(1);
    for (offset, field) in [
        (24, limits.max_resource_bytes),
        (32, limits.max_staging_bytes),
        (40, limits.max_resident_bytes),
        (48, limits.max_retiring_bytes),
        (56, limits.max_session_retiring_bytes),
    ] {
        let bytes = valid[body + offset..body + offset + 8].try_into().unwrap();
        assert_eq!(u64::from_le_bytes(bytes), field, "offset {offset}");
    }
    for (offset, value) in [
        (32, 4 * MIB - 4),
        (40, 4 * MIB - 4),
        (48, 4 * MIB - 4),
        (56, 16 * MIB - 4),
    ] {
        let mut object = valid.clone();
        object[body + offset..body + offset + 8].copy_from_slice(&value.to_le_bytes());
        assert!(
            decode_shell_file_limits(&object).is_err(),
            "offset {offset} value {value} decoded"
        );
    }
}
