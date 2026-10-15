//! Supplied admission and paused client I/O, not process scheduling or timing.
use super::{Client, WAIT};
use sophia_protocol::*;
use sophia_runtime::*;
use std::sync::mpsc;
use std::time::Instant;

const MIB: u64 = 1024 * 1024;

fn limits(epoch: u64, first: bool) -> ContentLimits {
    let mut value = ContentLimits::prototype(ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    });
    value.max_staging_bytes = 4 * MIB;
    value.max_resident_bytes = if first { 12 * MIB } else { 8 * MIB };
    value.max_retiring_bytes = 8 * MIB;
    value
}

struct Peer {
    host: ShellComponentTransport,
    limits: ContentLimits,
    commands: mpsc::Sender<Option<(u64, u8)>>,
    replies: mpsc::Receiver<Vec<ShellContentRecord>>,
    thread: Option<std::thread::JoinHandle<()>>,
    directory: std::path::PathBuf,
}

impl Peer {
    fn connect(registry: &mut ContentEpochRegistry, files: bool, limits: ContentLimits) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "comparator-component-{}-{files}-{}",
            std::process::id(),
            limits.grant.connection_epoch
        ));
        let mut host = ShellComponentTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        host.authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
        host.reserve_content(registry, limits.clone()).unwrap();
        let socket = host.socket_path().to_owned();
        let expected = limits.clone();
        let (commands, requests) = mpsc::channel::<Option<(u64, u8)>>();
        let (responses, replies) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut client = Client::connect(&socket, files);
            let first = client.receive();
            assert_eq!(first, ShellContentRecord::Limits(expected.clone()));
            responses.send(vec![first]).unwrap();
            // No I/O occurs between commands, including SDK polling. A peer
            // with no pending command is a deliberately stopped reader.
            while let Some((id, color)) = requests.recv_timeout(WAIT).unwrap() {
                let grant = expected.grant;
                let resource = ContentResourceId { id, generation: 1 };
                client.send(
                    id,
                    &ShellContentRecord::ResourceBegin(ContentResourceBegin {
                        grant,
                        resource,
                        width_px: 1,
                        height_px: 1,
                        rendered_scale_numerator: 1,
                        rendered_scale_denominator: 1,
                        pixel_format: 1,
                        chunk_count: 1,
                        total_bytes: 4,
                    }),
                );
                client.send(
                    id,
                    &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                        grant,
                        resource,
                        ordinal: 0,
                        offset: 0,
                        bytes: vec![color, 0, 0, 255],
                    }),
                );
                client.send(
                    id,
                    &ShellContentRecord::ResourceEnd(ContentResourceEnd {
                        grant,
                        resource,
                        total_bytes: 4,
                        chunk_count: 1,
                    }),
                );
                let records = vec![client.receive(), client.receive()];
                for (record, status) in records.iter().zip([1, 2]) {
                    assert!(matches!(record, ShellContentRecord::ResourceStatus(s)
                        if s.grant == grant && s.resource == resource && s.status == status));
                }
                responses.send(records).unwrap();
            }
        });
        let policy = ShellContentAdmissionPolicy::Granted {
            discrete_input: false,
        };
        if files {
            host.begin_file_negotiation(registry, limits.grant.connection_epoch, WAIT, policy)
        } else {
            host.begin_negotiation(registry, limits.grant.connection_epoch, WAIT, policy)
        }
        .unwrap();
        let start = Instant::now();
        while host.poll_negotiation(registry, 65536).unwrap().is_none() {
            assert!(start.elapsed() < WAIT);
            std::thread::yield_now();
        }
        loop {
            host.service_owner_turn(registry).unwrap();
            if let Ok(records) = replies.try_recv() {
                assert_eq!(records, vec![ShellContentRecord::Limits(limits.clone())]);
                break;
            }
            assert!(start.elapsed() < WAIT);
            std::thread::yield_now();
        }
        Self {
            host,
            limits,
            commands,
            replies,
            thread: Some(thread),
            directory,
        }
    }

    fn disconnect(&mut self, registry: &mut ContentEpochRegistry) {
        self.host.disconnect(registry).unwrap();
        if let Some(thread) = self.thread.take() {
            self.commands.send(None).unwrap();
            thread.join().unwrap();
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        if self.directory.exists() {
            std::fs::remove_dir_all(&self.directory).unwrap();
        }
    }
}

fn uploads(
    peers: &mut [Peer],
    registry: &mut ContentEpochRegistry,
    ids: &[(usize, u64)],
) -> Vec<ShellContentRecord> {
    for &(index, id) in ids {
        peers[index]
            .commands
            .send(Some((id, 30 + index as u8)))
            .unwrap();
    }
    let mut results = vec![None; ids.len()];
    let start = Instant::now();
    while results.iter().any(Option::is_none) {
        // One real bounded wire turn for each owner; a stopped reader is
        // still visited alongside the peers whose uploads must advance.
        for peer in peers.iter_mut().filter(|p| p.thread.is_some()) {
            peer.host.service_owner_turn(registry).unwrap();
            peer.host.service_content_resources(registry, 0).unwrap();
            let accounting = peer.host.content_accounting(registry);
            assert!(accounting.response_records <= peer.limits.max_control_records as usize);
            assert!(accounting.response_bytes <= peer.limits.max_output_queue_bytes as usize);
        }
        for (slot, &(index, _)) in ids.iter().enumerate() {
            if results[slot].is_none() {
                results[slot] = peers[index].replies.try_recv().ok();
            }
        }
        assert!(start.elapsed() < WAIT, "neighbor upload stalled");
        std::thread::yield_now();
    }
    for &(index, id) in ids {
        let peer = &peers[index];
        let pixels = peer
            .host
            .lease_content_resource(
                registry,
                peer.limits.grant,
                ContentResourceId { id, generation: 1 },
            )
            .unwrap();
        assert_eq!(pixels.bytes(), [30 + index as u8, 0, 0, 255]);
    }
    results.into_iter().flat_map(Option::unwrap).collect()
}

fn roundtrip(files: bool) -> Vec<ShellContentRecord> {
    let mut registry = ContentEpochRegistry::with_active_capacity(64 * MIB, 3).unwrap();
    let mut peers: Vec<_> = (1..=3)
        .map(|epoch| Peer::connect(&mut registry, files, limits(epoch, epoch == 1)))
        .collect();
    assert_eq!(registry.reserved_bytes(), 64 * MIB);
    let mut report = uploads(&mut peers, &mut registry, &[(0, 1), (1, 1), (2, 1)]);
    let old_grant = peers[2].limits.grant;
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let held = peers[2]
        .host
        .lease_content_resource(&registry, old_grant, resource)
        .unwrap();
    assert!(
        peers[0]
            .host
            .lease_content_resource(&registry, old_grant, resource)
            .is_err()
    );

    let mut saturated_at = None;
    for generation in 1..=10000 {
        let result = peers[2].host.publish_content_output_facts(
            &mut registry,
            TransactionId::from_raw(generation),
            generation,
            vec![ContentOutputFactsEntry {
                output: ContentOutputId {
                    id: 1,
                    generation: 1,
                },
                local_width: 64,
                local_height: 64,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        );
        match result {
            Ok(()) => peers[2].host.service_owner_turn(&mut registry).unwrap(),
            Err(ShellTransportError::ContentQueueSaturated) => {
                saturated_at = Some(generation);
                break;
            }
            other => panic!("unexpected publication result: {other:?}"),
        }
    }
    assert!(
        saturated_at.is_some(),
        "stopped reader never reached backpressure"
    );
    eprintln!(
        "files={files} output-facts backpressure at {}",
        saturated_at.unwrap()
    );
    report.extend(uploads(&mut peers, &mut registry, &[(0, 2), (1, 2)]));
    peers[2].disconnect(&mut registry);
    assert_eq!(registry.retired_bytes(), 4);
    assert_eq!(
        registry.admit(limits(4, false)),
        Err(ContentStoreError::Budget)
    );
    report.extend(uploads(&mut peers, &mut registry, &[(0, 3), (1, 3)]));
    assert_eq!(held.bytes(), [32, 0, 0, 255]);
    drop(held);
    registry.collect();
    assert_eq!(registry.retired_bytes(), 0);
    let old = peers.pop().unwrap();
    peers.push(Peer::connect(&mut registry, files, limits(4, false)));
    let new_grant = peers[2].limits.grant;
    report.extend(uploads(
        &mut peers,
        &mut registry,
        &[(0, 4), (1, 4), (2, 1)],
    ));
    assert!(
        old.host
            .lease_content_resource(&registry, new_grant, resource)
            .is_err()
    );
    assert!(
        peers[2]
            .host
            .lease_content_resource(&registry, old_grant, resource)
            .is_err()
    );
    assert_eq!(peers[0].host.content_grant(), Some(limits(1, true).grant));
    assert_eq!(peers[1].host.content_grant(), Some(limits(2, false).grant));
    for peer in &mut peers {
        peer.disconnect(&mut registry);
    }
    registry.collect();
    assert!(registry.accounting().quiescent());
    report
}

#[test]
fn stopped_reader_and_replacement_preserve_two_neighbors_on_both_wires() {
    assert_eq!(roundtrip(true), roundtrip(false));
}
