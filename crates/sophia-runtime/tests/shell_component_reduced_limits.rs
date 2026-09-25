//! t100 reduced initial welcome limits. A reconnecting component may be
//! admitted with a tightened but coherent profile: staging, resident and
//! retiring reduced to 4/8/4 MiB with the per-resource bound unchanged at
//! 4 MiB. These controls use the real component transport, the real epoch
//! registry and the real Rust client library (the one Lom pins), not the
//! Session budget selector.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{ContentLifecycle, ShellClientOptions, ShellConnection};

const MIB: u64 = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

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

fn transport(directory: &std::path::Path) -> ShellComponentTransport {
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    transport
}

struct Directory(std::path::PathBuf);

impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "shell-reduced-limits-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn next_record(client: &mut ShellConnection) -> ShellContentRecord {
    let start = Instant::now();
    loop {
        if let Some((_, record)) = client
            .poll_content()
            .unwrap_or_else(|error| panic!("welcome phase: client poll failed: {error:?}"))
        {
            return record;
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "welcome phase: no content record after {elapsed:?}"
        );
        std::thread::yield_now();
    }
}

/// Connect a real client and return it with the first content record it
/// actually received after negotiation.
fn connect(
    transport: &mut ShellComponentTransport,
    registry: &mut ContentEpochRegistry,
    epoch: u64,
) -> (ShellConnection, ShellContentRecord) {
    let socket = transport.socket_path().to_owned();
    let peer = std::thread::spawn(move || {
        let mut client = ShellConnection::connect(
            socket,
            ShellClientOptions {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
                handshake_timeout: Duration::from_secs(2),
            },
        )
        .unwrap();
        let record = next_record(&mut client);
        (client, record)
    });
    transport
        .accept_and_negotiate_with_content_policy(
            registry,
            epoch,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    peer.join().unwrap()
}

/// One full-size resource: 1024 x 1024 ARGB is exactly the 4 MiB bound.
fn full_size(grant: ContentGrant, id: u64, value: u8) -> Vec<ShellContentRecord> {
    let resource = ContentResourceId { id, generation: 1 };
    // The protocol's own chunk rule: whole rows per chunk payload.
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
    let layout = begin.layout(&floor(grant.connection_epoch)).unwrap();
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
fn floor_limits_reach_the_real_client_as_its_first_welcome_record() {
    let directory = Directory::new();
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut transport = transport(&directory.0);
    let limits = floor(1);
    transport
        .reserve_content(&mut registry, limits.clone())
        .unwrap();
    // The registry charges exactly the reduced profile, not the prototype.
    assert_eq!(registry.reserved_bytes(), 16 * MIB);
    assert_eq!(registry.reserved_backing_bytes(), 12 * MIB);
    let (client, received) = connect(&mut transport, &mut registry, 1);
    assert_eq!(received, ShellContentRecord::Limits(limits.clone()));
    let ShellContentRecord::Limits(received) = received else {
        unreachable!()
    };
    // What a client does with the first record: Lom validates and builds
    // its lifecycle from it; both accept the reduced profile.
    received.validate().unwrap();
    assert_eq!(received.max_resource_bytes, 4 * MIB);
    assert_eq!(
        received.max_session_retiring_bytes,
        ContentLimits::prototype(received.grant).max_session_retiring_bytes
    );
    ContentLifecycle::new(received).unwrap();
    drop(client);
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    assert!(registry.accounting().quiescent());
}

#[test]
fn floor_limits_hold_two_full_size_resources_resident() {
    let directory = Directory::new();
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut transport = transport(&directory.0);
    let limits = floor(1);
    transport
        .reserve_content(&mut registry, limits.clone())
        .unwrap();
    let (mut client, _) = connect(&mut transport, &mut registry, 1);
    let grant = limits.grant;
    // The client writes on its own thread so a full socket cannot stall the
    // owner's service loop below; it returns the statuses it read.
    let peer = std::thread::spawn(move || {
        let mut pending: std::collections::VecDeque<_> = [(1, 30), (2, 90)]
            .into_iter()
            .flat_map(|(id, value)| {
                full_size(grant, id, value)
                    .into_iter()
                    .map(move |record| (TransactionId::from_raw(id), record))
            })
            .collect();
        let mut statuses = Vec::new();
        let start = Instant::now();
        let mut last_progress = (Duration::ZERO, String::from("none"));
        while !pending.is_empty() || statuses.len() != 4 {
            // A saturated outbox is the client's own backpressure: service
            // the socket and offer the same record again, as Lom does on its
            // next turn.
            if let Some((transaction, record)) = pending.front() {
                match client.enqueue_content(*transaction, record) {
                    Ok(()) => {
                        last_progress = (
                            start.elapsed(),
                            format!("enqueued a record of transaction {transaction:?}"),
                        );
                        pending.pop_front();
                    }
                    Err(sophia_shell_client::ShellClientError::QueueSaturated) => {}
                    Err(error) => panic!(
                        "upload phase: enqueue failed with {error:?}; {} records left; statuses {statuses:?}",
                        pending.len()
                    ),
                }
            }
            client.poll_io().unwrap_or_else(|error| {
                panic!(
                    "upload phase: client I/O failed with {error:?}; {} records left; statuses {statuses:?}",
                    pending.len()
                )
            });
            let polled = client.poll_content().unwrap_or_else(|error| {
                panic!(
                    "upload phase: client poll failed with {error:?}; {} records left; statuses {statuses:?}",
                    pending.len()
                )
            });
            if let Some((_, record)) = polled {
                let ShellContentRecord::ResourceStatus(status) = record else {
                    panic!("upload phase: expected a resource status, got {record:?}");
                };
                // Status 1 is begun and 2 accepted; anything else is a
                // terminal refusal, reported at once rather than waited out.
                assert!(
                    matches!(status.status, 1 | 2),
                    "upload phase: resource {} refused with status {} reason {}; {} records left; statuses {statuses:?}",
                    status.resource.id,
                    status.status,
                    status.reason,
                    pending.len()
                );
                statuses.push((status.resource.id, status.status));
                last_progress = (
                    start.elapsed(),
                    format!(
                        "status {} for resource {}",
                        status.status, status.resource.id
                    ),
                );
            }
            let elapsed = start.elapsed();
            assert!(
                elapsed < Duration::from_secs(15),
                "upload phase: deadline after {elapsed:?}; {} records left to enqueue; statuses {statuses:?}; last progress '{}' at {:?}",
                pending.len(),
                last_progress.1,
                last_progress.0
            );
        }
        (client, statuses)
    });
    let start = Instant::now();
    let mut serviced = 0;
    let mut owner_error = None;
    while !peer.is_finished() {
        match transport.service_content_resources(&mut registry, 0) {
            Ok(count) => serviced += count,
            // A peer that failed closes its socket; its own diagnostic, not
            // the resulting broken pipe, is the cause to report.
            Err(error) => {
                owner_error = Some(error);
                break;
            }
        }
        let elapsed = start.elapsed();
        // Longer than the peer's own bound, so the peer's diagnostic wins.
        assert!(
            elapsed < Duration::from_secs(20),
            "owner service phase: peer still running after {elapsed:?}; {serviced} records serviced"
        );
        std::thread::yield_now();
    }
    // Re-raise the peer's own diagnostic rather than an opaque join error.
    let (client, statuses) = peer
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    if let Some(error) = owner_error {
        panic!(
            "owner service phase: {error:?} after {serviced} records; peer statuses {statuses:?}"
        );
    }
    assert_eq!(statuses, [(1, 1), (1, 2), (2, 1), (2, 2)]);
    let first = transport
        .lease_content_resource(
            &registry,
            grant,
            ContentResourceId {
                id: 1,
                generation: 1,
            },
        )
        .unwrap();
    let second = transport
        .lease_content_resource(
            &registry,
            grant,
            ContentResourceId {
                id: 2,
                generation: 1,
            },
        )
        .unwrap();
    assert_eq!(&first.bytes()[..4], &[30, 0, 0, 255]);
    assert_eq!(&second.bytes()[..4], &[90, 0, 0, 255]);
    let accounting = registry.accounting();
    assert_eq!(accounting.memory.resident, 8 * MIB);
    assert_eq!(accounting.memory.staging, 0);
    // A third full-size resource cannot fit: the reduced resident bound is
    // exactly two, which is what the floor promises and no more.
    assert!(accounting.memory.resident + 4 * MIB > limits.max_resident_bytes);
    drop((first, second, client));
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    assert!(registry.accounting().quiescent());
}

#[test]
fn incoherent_reduced_profiles_are_refused_before_any_reservation() {
    let directory = Directory::new();
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut transport = transport(&directory.0);
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
            transport.reserve_content(&mut registry, limits),
            Err(ShellTransportError::ContentStore(error)) if error == expected
        ));
        assert_eq!(registry.reserved_bytes(), 0);
        assert_eq!(registry.accounting().active_epochs, 0);
    }
    // No refusal published a watermark: the same epoch still admits.
    transport.reserve_content(&mut registry, floor(1)).unwrap();
    assert_eq!(registry.reserved_bytes(), 16 * MIB);
}

#[test]
fn a_client_refuses_an_incoherent_welcome_limits_record() {
    let valid = encode_shell_content_frame(
        TransactionId::from_raw(0),
        &ShellContentRecord::Limits(floor(1)),
    )
    .unwrap();
    let (_, decoded) = decode_shell_content_frame(&valid).unwrap();
    assert_eq!(decoded, ShellContentRecord::Limits(floor(1)));
    // The limits payload is the frame's last 264 bytes: resource bound at
    // 24, staging 32, resident 40, retiring 48, session bound 56.
    let payload = valid.len() - 264;
    for (offset, value) in [
        (32, 4 * MIB - 4),
        (40, 4 * MIB - 4),
        (48, 4 * MIB - 4),
        (56, 16 * MIB - 4),
    ] {
        let mut frame = valid.clone();
        frame[payload + offset..payload + offset + 8].copy_from_slice(&value.to_le_bytes());
        assert!(
            decode_shell_content_frame(&frame).is_err(),
            "offset {offset} value {value} decoded"
        );
    }
}
