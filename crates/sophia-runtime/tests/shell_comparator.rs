//! Comparison-tree feasibility, not timing acceptance. Both wires feed the
//! current resource owner with identical typed records and supplied admission.
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{ShellClientError, ShellClientOptions, ShellConnection};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);
#[path = "support/comparator_components.rs"]
mod components;
#[path = "support/comparator_measurement.rs"]
mod measurement;
enum Client {
    Files(Box<ShellConnection>),
    Ipc(UnixStream),
}
impl Client {
    fn connect(path: &Path, files: bool) -> Self {
        Self::connect_with_input(path, files, false)
    }
    fn connect_with_input(path: &Path, files: bool, input: bool) -> Self {
        let caps = SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | if input {
                SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
            } else {
                0
            };
        if files {
            Self::Files(Box::new(
                ShellConnection::connect_files(
                    path,
                    ShellClientOptions {
                        minimum_revision: 5,
                        maximum_revision: 6,
                        required_capabilities: caps,
                        handshake_timeout: WAIT,
                    },
                )
                .unwrap(),
            ))
        } else {
            let mut stream = UnixStream::connect(path).unwrap();
            stream.set_read_timeout(Some(WAIT)).unwrap();
            stream.set_write_timeout(Some(WAIT)).unwrap();
            stream
                .write_all(
                    &encode_shell_v1_client_hello_frame(ShellV1ClientHello {
                        minimum_revision: 5,
                        maximum_revision: 6,
                        required_capabilities: caps,
                    })
                    .unwrap(),
                )
                .unwrap();
            let welcome = decode_shell_v1_server_welcome_frame(&frame(&mut stream)).unwrap();
            assert_eq!(welcome.selected_revision, 6);
            Self::Ipc(stream)
        }
    }
    fn send(&mut self, id: u64, record: &ShellContentRecord) {
        let tx = TransactionId::from_raw(id);
        match self {
            Self::Files(client) => {
                let deadline = Instant::now() + WAIT;
                loop {
                    match client.enqueue_content(tx, record) {
                        Ok(()) => break,
                        Err(ShellClientError::QueueSaturated) => {
                            assert!(Instant::now() < deadline, "file send deadline");
                            client
                                .wait_for_io(deadline.saturating_duration_since(Instant::now()))
                                .unwrap();
                        }
                        Err(error) => panic!("file enqueue: {error:?}"),
                    }
                }
                client.poll_io().unwrap();
            }
            Self::Ipc(stream) => stream
                .write_all(&encode_shell_content_frame(tx, record).unwrap())
                .unwrap(),
        }
    }
    fn receive(&mut self) -> ShellContentRecord {
        match self {
            Self::Ipc(stream) => decode_shell_content_frame(&frame(stream)).unwrap().1,
            Self::Files(client) => {
                let deadline = Instant::now() + WAIT;
                loop {
                    if let Some((_, record)) = client.poll_content().unwrap() {
                        return record;
                    }
                    assert!(Instant::now() < deadline, "file response deadline");
                    client
                        .wait_for_io(deadline.saturating_duration_since(Instant::now()))
                        .unwrap();
                }
            }
        }
    }
    fn candidate(&mut self, records: &[ShellContentRecord]) {
        match self {
            Self::Files(client) => {
                client
                    .enqueue_content_group(TransactionId::from_raw(20), records)
                    .unwrap();
            }
            Self::Ipc(_) => {
                for (index, record) in records.iter().enumerate() {
                    self.send(20 + index as u64, record);
                }
            }
        }
    }
    fn wait_for_owner(&mut self, done: mpsc::Receiver<()>) {
        let deadline = Instant::now() + WAIT;
        loop {
            match done.try_recv() {
                Ok(()) => return,
                Err(mpsc::TryRecvError::Empty) => {}
                Err(error) => panic!("owner ended: {error}"),
            }
            if let Self::Files(client) = self {
                client.poll_io().unwrap();
            }
            assert!(Instant::now() < deadline, "owner completion deadline");
            std::thread::yield_now();
        }
    }
}
fn frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header).unwrap();
    let size = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    assert!(size <= SOPHIA_IPC_MAX_PAYLOAD_LEN);
    let mut bytes = header.to_vec();
    bytes.resize(SOPHIA_IPC_HEADER_LEN + size, 0);
    stream
        .read_exact(&mut bytes[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    bytes
}

fn exchange(files: bool, width: u32, height: u32) -> Vec<ShellContentRecord> {
    measured_exchange(files, width, height).0
}

fn measured_exchange(
    files: bool,
    width: u32,
    height: u32,
) -> (Vec<ShellContentRecord>, measurement::Sample) {
    let root = std::env::temp_dir().join(format!(
        "shell-comparator-{}-{files}-{width}-{height}",
        std::process::id()
    ));
    let mut host = ShellComponentTransport::bind_for_supervised_uid(
        &root,
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
    let mut epochs = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    host.reserve_content(&mut epochs, ContentLimits::prototype(grant))
        .unwrap();
    let socket = host.socket_path().to_owned();
    let (done_tx, done_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let bytes = (0..width as usize * height as usize * 4)
        .map(|n| if n % 4 == 3 { 255 } else { (n % 128) as u8 })
        .collect::<Vec<_>>();
    let expected = bytes.clone();
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let client = std::thread::spawn(move || {
        let mut client = Client::connect(&socket, files);
        let first = client.receive();
        let ShellContentRecord::Limits(limits) = &first else {
            panic!("missing limits")
        };
        assert_eq!(limits.grant, grant);
        let chunk_bytes = (limits.max_chunk_bytes / (width * 4) * (width * 4)) as usize;
        let chunks = bytes.len().div_ceil(chunk_bytes) as u32;
        let mut report = vec![first];
        let measurement = measurement::Window::begin();
        client.send(
            1,
            &ShellContentRecord::ResourceBegin(ContentResourceBegin {
                grant,
                resource,
                width_px: width,
                height_px: height,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: chunks,
                total_bytes: bytes.len() as u64,
            }),
        );
        let begin = client.receive();
        assert!(
            matches!(&begin, ShellContentRecord::ResourceStatus(s) if s.status == 1),
            "files={files} size={width}x{height} response={begin:?}"
        );
        report.push(begin);
        for (ordinal, chunk) in bytes.chunks(chunk_bytes).enumerate() {
            client.send(
                2 + ordinal as u64,
                &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                    grant,
                    resource,
                    ordinal: ordinal as u32,
                    offset: ordinal as u64 * chunk_bytes as u64,
                    bytes: chunk.to_vec(),
                }),
            );
        }
        client.send(
            1000,
            &ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant,
                resource,
                total_bytes: bytes.len() as u64,
                chunk_count: chunks,
            }),
        );
        let accepted = client.receive();
        let sample = measurement.end();
        assert!(
            matches!(&accepted, ShellContentRecord::ResourceStatus(s) if s.status == 2),
            "files={files} size={width}x{height} response={accepted:?}"
        );
        report.push(accepted);
        done_tx.send(()).unwrap();
        release_rx.recv_timeout(WAIT).unwrap();
        client.send(
            1001,
            &ShellContentRecord::ResourceRetire(ContentResourceRetire { grant, resource }),
        );
        let released = client.receive();
        assert!(matches!(&released, ShellContentRecord::ResourceReleased(r) if r.reason == 0));
        report.push(released);
        // Retirement must not make the same generation admissible again.
        client.send(
            1002,
            &ShellContentRecord::ResourceBegin(ContentResourceBegin {
                grant,
                resource,
                width_px: width,
                height_px: height,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: chunks,
                total_bytes: bytes.len() as u64,
            }),
        );
        let stale = client.receive();
        assert!(matches!(&stale, ShellContentRecord::ResourceStatus(s)
            if s.status == 3 && s.reason == ContentReason::Stale as u16));
        report.push(stale);
        done_tx.send(()).unwrap();
        release_rx.recv_timeout(WAIT).unwrap();
        (report, sample)
    });
    let policy = ShellContentAdmissionPolicy::Granted {
        discrete_input: false,
    };
    if files {
        host.begin_file_negotiation(&epochs, 1, WAIT, policy)
            .unwrap();
    } else {
        host.begin_negotiation(&epochs, 1, WAIT, policy).unwrap();
    }
    let start = Instant::now();
    while host.poll_negotiation(&mut epochs, 65536).unwrap().is_none() {
        assert!(start.elapsed() < WAIT);
        std::thread::yield_now();
    }
    let mut phase = 0;
    while phase < 2 {
        host.service_owner_turn(&mut epochs).unwrap();
        host.service_content_resources(&mut epochs, start.elapsed().as_millis() as u64)
            .unwrap();
        if done_rx.try_recv().is_ok() {
            if phase == 0 {
                let lease = host
                    .lease_content_resource(&epochs, grant, resource)
                    .unwrap();
                assert_eq!(lease.bytes(), expected);
                drop(lease);
                release_tx.send(()).unwrap();
            }
            phase += 1;
        }
        assert!(start.elapsed() < WAIT, "owner deadline");
        std::thread::yield_now();
    }
    host.disconnect(&mut epochs).unwrap();
    release_tx.send(()).unwrap();
    let report = client.join().unwrap();
    epochs.collect();
    assert!(epochs.accounting().quiescent());
    drop(host);
    if root.exists() {
        std::fs::remove_dir_all(root).unwrap();
    }
    report
}

#[test]
fn current_resource_owner_has_equal_results_on_both_wires() {
    for (width, height) in [(2, 1), (1920, 24), (1024, 1024)] {
        assert_eq!(
            exchange(true, width, height),
            exchange(false, width, height)
        );
    }
}

fn resource_id(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}

fn small_upload(grant: ContentGrant, resource: ContentResourceId) -> [ShellContentRecord; 3] {
    [
        ShellContentRecord::ResourceBegin(ContentResourceBegin {
            grant,
            resource,
            width_px: 2,
            height_px: 1,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 8,
        }),
        ShellContentRecord::ResourceChunk(ContentResourceChunk {
            grant,
            resource,
            ordinal: 0,
            offset: 0,
            bytes: vec![0, 0, 255, 255, 0, 128, 0, 128],
        }),
        ShellContentRecord::ResourceEnd(ContentResourceEnd {
            grant,
            resource,
            total_bytes: 8,
            chunk_count: 1,
        }),
    ]
}

fn candidate(
    grant: ContentGrant,
    permit: &ContentFramePermit,
    facts_generation: u64,
    allocation: &ContentAllocationResult,
    resource: ContentResourceId,
) -> [ShellContentRecord; 3] {
    let generation = 1;
    [
        ShellContentRecord::CandidateBegin(ContentCandidateBegin {
            grant,
            candidate_generation: generation,
            output: permit.output,
            facts_generation,
            pacing_permit: permit.permit_id,
            interaction_generation: 4,
            surface_count: 1,
            placement_count: 1,
            target_count: 1,
        }),
        ShellContentRecord::CandidateChunk(ContentCandidateChunk {
            grant,
            candidate_generation: generation,
            chunk_ordinal: 0,
            surfaces: vec![ContentSurface {
                allocation: allocation.allocation,
                scale_generation: allocation.scale_generation,
                role: 1,
                edge: 1,
                margins: ContentMargins::default(),
                reservation_extent: 24,
                parent_surface_index: u16::MAX,
                anchor_parent_rect: ContentPixelRect::default(),
            }],
            placements: vec![ContentPlacement {
                resource,
                surface_index: 0,
                destination_x_px: 3,
                destination_y_px: 4,
            }],
            targets: vec![ContentTarget {
                surface_index: 0,
                action_kind: 1,
                target_id: 1,
                target_generation: 1,
                action_id: 1,
                bounds_px: ContentPixelRect {
                    x: 3,
                    y: 4,
                    width: 2,
                    height: 1,
                },
            }],
        }),
        ShellContentRecord::CandidateEnd(ContentCandidateEnd {
            grant,
            candidate_generation: generation,
            surface_count: 1,
            placement_count: 1,
            target_count: 1,
        }),
    ]
}

fn allocation_request(grant: ContentGrant, output: ContentOutputId) -> ShellContentRecord {
    ShellContentRecord::AllocationRequest(ContentAllocationRequest {
        grant,
        output,
        allocation_request_id: 1,
        operation: 1,
        role: 1,
        edge: 1,
        prior: ContentAllocationId::default(),
        parent: ContentAllocationId::default(),
        parent_presentation_epoch: 0,
        anchor_parent_rect: ContentPixelRect::default(),
        desired_width: 64,
        desired_height: 32,
        margins: ContentMargins::default(),
    })
}

fn snapshot(output: ContentOutputId, allocation: ContentAllocationId) -> ContentAllocationSnapshot {
    ContentAllocationSnapshot {
        native_opening: None,
        output,
        allocation,
        scale_generation: 5,
        scale_numerator: 1,
        scale_denominator: 1,
        role: 1,
        edge: 1,
        margins: ContentMargins::default(),
        logical: ContentLogicalRect {
            x: 0,
            y: 0,
            width: 64,
            height: 32,
        },
        pixel: ContentPixelRect {
            x: 0,
            y: 0,
            width: 64,
            height: 32,
        },
        parent: ContentAllocationId::default(),
        anchor_parent_rect: ContentPixelRect::default(),
        allowed_reservation_extent: 32,
    }
}

fn candidate_roundtrip(files: bool, renderer_failed: bool) -> Vec<ShellContentRecord> {
    let root = std::env::temp_dir().join(format!(
        "candidate-comparator-{}-{files}-{renderer_failed}",
        std::process::id()
    ));
    let mut session =
        ShellSessionTransport::bind_for_supervised_uid(&root, rustix::process::geteuid().as_raw())
            .unwrap();
    session
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    let socket = session.socket_path().to_path_buf();
    let (uploaded_tx, uploaded_rx) = mpsc::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let client = std::thread::spawn(move || {
        let mut client = Client::connect_with_input(&socket, files, true);
        let mut report = Vec::new();
        let ShellContentRecord::Limits(limits) = client.receive() else {
            panic!("expected limits");
        };
        let ShellContentRecord::OutputFacts(facts) = client.receive() else {
            panic!("expected output facts");
        };
        let output = facts.outputs[0].output;
        report.push(ShellContentRecord::Limits(limits.clone()));
        report.push(ShellContentRecord::OutputFacts(facts.clone()));
        client.send(6, &allocation_request(limits.grant, output));
        let ShellContentRecord::AllocationResult(allocation) = client.receive() else {
            panic!("expected allocation result");
        };
        assert_eq!(allocation.status, 1);
        report.push(ShellContentRecord::AllocationResult(allocation.clone()));
        let resource = resource_id(1);
        for record in small_upload(limits.grant, resource) {
            client.send(7, &record);
        }
        for expected in [1, 2] {
            let status = client.receive();
            assert!(
                matches!(&status, ShellContentRecord::ResourceStatus(s) if s.status == expected)
            );
            report.push(status);
        }
        client.send(
            9,
            &ShellContentRecord::FrameDemand(ContentFrameDemand {
                grant: limits.grant,
                output,
                allocation: ContentAllocationId::default(),
                demand_id: 1,
                reason: 1,
            }),
        );
        uploaded_tx.send(()).unwrap();
        let ShellContentRecord::FramePermit(permit) = client.receive() else {
            panic!("expected frame permit");
        };
        assert_eq!(permit.state, 1);
        report.push(ShellContentRecord::FramePermit(permit.clone()));
        let records = candidate(
            limits.grant,
            &permit,
            facts.facts_generation,
            &allocation,
            resource,
        );
        client.candidate(&records);
        let mut outcomes = Vec::new();
        while outcomes.len() < 2 {
            let record = client.receive();
            let ShellContentRecord::CandidateOutcome(outcome) = record else {
                panic!("unexpected record while waiting for outcome: {record:?}");
            };
            if renderer_failed && outcome.kind == 3 {
                assert_eq!(outcome.reason, ContentReason::RendererFailed as u16);
            }
            outcomes.push((outcome.kind, outcome.presentation_epoch));
            report.push(ShellContentRecord::CandidateOutcome(outcome));
        }
        assert_eq!(
            outcomes,
            if renderer_failed {
                vec![(1, 0), (3, 0)]
            } else {
                vec![(1, 0), (2, 9)]
            }
        );
        if !renderer_failed {
            let ShellContentRecord::Action(action) = client.receive() else {
                panic!("missing action");
            };
            let ack = ContentActionAck {
                grant: action.grant,
                output: action.output,
                candidate_generation: action.candidate_generation,
                presentation_epoch: action.presentation_epoch,
                interaction_generation: action.interaction_generation,
                allocation: action.allocation,
                target_id: action.target_id,
                target_generation: action.target_generation,
                action_id: action.action_id,
                event_id: action.event_id,
                disposition: 1,
            };
            report.push(ShellContentRecord::Action(action));
            report.push(ShellContentRecord::ActionAck(ack.clone()));
            client.send(51, &ShellContentRecord::ActionAck(ack));
        }
        client.wait_for_owner(finish_rx);
        report
    });

    let policy = ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    };
    let welcome = if files {
        session.accept_files_with_content_policy(1, WAIT, policy)
    } else {
        session.accept_and_negotiate_with_content_policy(1, WAIT, policy)
    }
    .unwrap();
    let grant = session.content_grant().unwrap();
    let start = Instant::now();
    let output = ContentOutputId {
        id: 2,
        generation: 1,
    };
    session
        .publish_content_output_facts(
            TransactionId::from_raw(5),
            3,
            vec![ContentOutputFactsEntry {
                output,
                local_width: 64,
                local_height: 64,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 5,
            }],
        )
        .unwrap();
    while session.next_content_allocation_request().is_none() {
        session
            .service_content_allocation_requests(&[], start.elapsed().as_millis() as u64)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    let allocation_id = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    session
        .grant_content_allocation(1, snapshot(output, allocation_id), &[])
        .unwrap();
    while uploaded_rx.try_recv().is_err() {
        session
            .service_content_resources(start.elapsed().as_millis() as u64)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    let allocations = session.content_allocation_snapshots();
    let context = ContentCandidateContext {
        output,
        facts_generation: 3,
        interaction_generation: 4,
        allocations: &allocations,
    };
    while session.next_content_demand().is_none() {
        session
            .service_content_demands(&[output], &allocations)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    session
        .grant_content_demand(TransactionId::from_raw(19), output, 1, 10)
        .unwrap();
    let mut processed = 0;
    while processed < 3 {
        processed += session
            .service_content_candidates(&[context], 11 + u64::try_from(processed).unwrap())
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    let render = session.begin_content_submission(output, 1, 20).unwrap();
    assert_eq!(
        render.resource(resource_id(1)).unwrap().bytes(),
        [0, 0, 255, 255, 0, 128, 0, 128]
    );
    session
        .content_prepared(grant, output, 1, 7, 8, 21)
        .unwrap();
    if renderer_failed {
        session.content_renderer_failed(grant, output, 1).unwrap();
    } else {
        session
            .content_presented(grant, output, 1, 9, 7, 8)
            .unwrap();
        // Supply the selected target identity: physical hit testing is a
        // separate native gate. The transport must preserve every field.
        let action = ContentAction {
            grant,
            output,
            candidate_generation: 1,
            presentation_epoch: 9,
            interaction_generation: 4,
            allocation: allocation_id,
            target_id: 1,
            target_generation: 1,
            action_id: 1,
            event_id: 11,
            kind: 1,
            reason: ContentReason::None as u16,
        };
        session
            .send_content_action(TransactionId::from_raw(50), &action)
            .unwrap();
        let (transaction, ack) = loop {
            if let Some(value) = session.poll_content_action_ack().unwrap() {
                break value;
            }
            assert!(start.elapsed() < WAIT);
            std::thread::yield_now();
        };
        assert_eq!(transaction, TransactionId::from_raw(51));
        assert_eq!(
            ack,
            ContentActionAck {
                grant,
                output,
                candidate_generation: 1,
                presentation_epoch: 9,
                interaction_generation: 4,
                allocation: allocation_id,
                target_id: 1,
                target_generation: 1,
                action_id: 1,
                event_id: 11,
                disposition: 1,
            }
        );
        session.retain_content_action_reservations(|_| false);
    }
    finish_tx.send(()).unwrap();
    while !client.is_finished() {
        session.poll_io().unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    session.disconnect().unwrap();
    let report = client.join().unwrap();
    // The GPU preflight's disconnect / held renderer / release sequence.
    // Neither repeated disconnect nor collection can release a live consumer.
    assert!(session.content_reserved_bytes() > 0);
    assert!(session.content_backing_reserved_bytes() > 0);
    session.disconnect().unwrap();
    assert_eq!(
        render.resource(resource_id(1)).unwrap().bytes(),
        [0, 0, 255, 255, 0, 128, 0, 128]
    );
    assert!(session.content_reserved_bytes() > 0);
    assert!(!session.content_accounting().quiescent());
    drop(render);
    session.disconnect().unwrap();
    assert_eq!(session.content_reserved_bytes(), 0);
    assert_eq!(session.content_backing_reserved_bytes(), 0);
    assert!(session.content_accounting().quiescent());
    session.disconnect().unwrap();
    assert!(session.content_accounting().quiescent());
    assert_eq!(welcome.selected_revision, 6);
    drop(session);
    if root.exists() {
        std::fs::remove_dir_all(root).unwrap();
    }
    report
}

#[test]
fn current_candidate_owner_has_equal_completion_and_lease_results() {
    for renderer_failed in [false, true] {
        assert_eq!(
            candidate_roundtrip(true, renderer_failed),
            candidate_roundtrip(false, renderer_failed)
        );
    }
}
