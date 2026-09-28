//! `sophia_shell_fs_v1` twins of `tests/shell_component_reduced_limits.rs`:
//! the t100 floor profile (staging, resident and retiring at 4/8/4 MiB with
//! the per-resource bound unchanged at 4 MiB) reaches the SDK's
//! `connect_files` client and holds two full-size resources that crossed the
//! file wire. The welcome-record refusal uses the file codec's Limits object.
#[allow(dead_code)] // Shared file-component fixture; this file drives only part of it.
#[path = "support/file_component.rs"]
mod file_component;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use file_component::*;
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{ContentLifecycle, ShellClientError};

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

/// One full-size resource: 1024 x 1024 ARGB is exactly the 4 MiB bound,
/// chunked by the protocol's own whole-row layout for these limits.
fn full_size(grant: ContentGrant, id: u64, value: u8) -> Vec<ShellContentRecord> {
    let resource = ContentResourceId { id, generation: 1 };
    let limits = floor(grant.connection_epoch);
    let payload = (limits.max_frame_payload - 48).min(limits.max_chunk_bytes);
    let begin = ContentResourceBegin {
        grant,
        resource,
        width_px: 1024,
        height_px: 1024,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1024_u32.div_ceil(payload / 4096),
        total_bytes: 4 * MIB,
    };
    let layout = begin.layout(&limits).unwrap();
    let mut records = vec![ShellContentRecord::ResourceBegin(begin)];
    let row_bytes = u64::from(layout.row_bytes);
    for ordinal in 0..layout.chunk_count {
        let offset = u64::from(ordinal) * u64::from(layout.rows_per_chunk) * row_bytes;
        let bytes = (4 * MIB - offset).min(u64::from(layout.rows_per_chunk) * row_bytes);
        let mut pixels = vec![0_u8; bytes as usize];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[value, 0, 0, 255]);
        }
        records.push(ShellContentRecord::ResourceChunk(ContentResourceChunk {
            grant,
            resource,
            ordinal,
            offset,
            bytes: pixels,
        }));
    }
    records.push(ShellContentRecord::ResourceEnd(ContentResourceEnd {
        grant,
        resource,
        total_bytes: 4 * MIB,
        chunk_count: layout.chunk_count,
    }));
    records
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
fn floor_limits_hold_two_full_size_file_uploads_resident() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut component = Component::new();
    let limits = floor(1);
    component
        .transport
        .reserve_content(&mut registry, limits.clone())
        .unwrap();
    component.connect(&mut registry, limits.clone());
    let mut client = component.client.take().unwrap();
    let grant = limits.grant;
    // The client writes on its own thread so its pipelined writes and the
    // owner's service turns progress independently; it returns its statuses.
    let peer = std::thread::spawn(move || {
        let mut pending: VecDeque<_> = [(1, 30), (2, 90)]
            .into_iter()
            .flat_map(|(id, value)| {
                full_size(grant, id, value)
                    .into_iter()
                    .map(move |record| (TransactionId::from_raw(id), record))
            })
            .collect();
        let mut statuses = Vec::new();
        let mut saturated = 0_usize;
        let start = Instant::now();
        while !pending.is_empty() || statuses.len() != 4 {
            // A saturated outbox is the client's own backpressure: service
            // the wire and offer the same record again on the next turn.
            if let Some((transaction, record)) = pending.front() {
                match client.enqueue_content(*transaction, record) {
                    Ok(()) => {
                        pending.pop_front();
                    }
                    Err(ShellClientError::QueueSaturated) => saturated += 1,
                    Err(error) => panic!(
                        "upload phase: enqueue failed with {error:?}; {} records left; statuses {statuses:?}",
                        pending.len()
                    ),
                }
            }
            if let Some((_, record)) = client.poll_content().unwrap() {
                let ShellContentRecord::ResourceStatus(status) = record else {
                    panic!("upload phase: expected a resource status, got {record:?}");
                };
                // 1 is begun and 2 accepted; anything else is a refusal,
                // reported at once rather than waited out.
                assert!(
                    matches!(status.status, 1 | 2),
                    "upload phase: resource {} refused with status {} reason {}",
                    status.resource.id,
                    status.status,
                    status.reason
                );
                statuses.push((status.resource.id, status.status));
            }
            let elapsed = start.elapsed();
            assert!(
                elapsed < Duration::from_secs(30),
                "upload phase: deadline after {elapsed:?}; {} records left; statuses {statuses:?}",
                pending.len()
            );
        }
        (client, statuses, saturated)
    });
    let start = Instant::now();
    let mut owner_error = None;
    while !peer.is_finished() {
        if let Err(error) = component
            .transport
            .service_content_resources(&mut registry, 0)
        {
            owner_error = Some(error);
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(40),
            "owner service phase: peer still running"
        );
        std::thread::yield_now();
    }
    let (client, statuses, saturated) = peer
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    if let Some(error) = owner_error {
        panic!("owner service phase: {error:?}; peer statuses {statuses:?}");
    }
    assert_eq!(statuses, [(1, 1), (1, 2), (2, 1), (2, 2)]);
    // 8 MiB cannot sit in the SDK's bounded outbox at once, so the client
    // really met its own backpressure and retried.
    assert!(saturated > 0, "the upload never met client backpressure");
    let first = component
        .transport
        .lease_content_resource(
            &registry,
            grant,
            ContentResourceId {
                id: 1,
                generation: 1,
            },
        )
        .unwrap();
    let second = component
        .transport
        .lease_content_resource(
            &registry,
            grant,
            ContentResourceId {
                id: 2,
                generation: 1,
            },
        )
        .unwrap();
    assert_eq!(first.bytes().len() as u64, 4 * MIB);
    assert!(first.bytes().chunks_exact(4).all(|p| p == [30, 0, 0, 255]));
    assert!(second.bytes().chunks_exact(4).all(|p| p == [90, 0, 0, 255]));
    let accounting = registry.accounting();
    assert_eq!(accounting.memory.resident, 8 * MIB);
    assert_eq!(accounting.memory.staging, 0);
    // A third full-size resource cannot fit: the reduced resident bound is
    // exactly two, which is what the floor promises and no more.
    assert!(accounting.memory.resident + 4 * MIB > limits.max_resident_bytes);
    drop((first, second, client));
    component.disconnect(&mut registry);
    registry.collect();
    assert!(registry.accounting().quiescent());
}

/// Wire-free: kept beside the file cases so this coverage outlives the
/// socket file that also carries it.
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
