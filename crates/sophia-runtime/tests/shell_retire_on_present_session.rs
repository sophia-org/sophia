//! W2 prototype, end to end in process: the real SDK client negotiates r12
//! with bit 17, uploads owned-ring resources and names each predecessor in
//! its successor's candidate; the runtime's file export commits the retire
//! at Presented and releases only once no render lease remains. Renderer
//! completions and authentication verdicts are supplied; there is no GPU.
#![cfg(feature = "experimental-shell-retire-on-present")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sophia_protocol::shell::content::ContentCandidateRetire;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{RetireOnPresent, RetireProgress, ShellClientOptions, ShellConnection};

#[allow(dead_code)]
#[path = "support/shell_required_image.rs"]
mod image;
#[allow(dead_code)]
#[path = "support/shell_sdk_image.rs"]
mod sdk_image;
#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;

#[path = "support/shell_retire_on_present_c.rs"]
mod c_peer;

static NEXT: AtomicU64 = AtomicU64::new(0);

const CONTENT: u64 =
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
const OWNED: u64 = 1 << 15;
const W2: u64 = 1 << 17;
const OUTPUT: ContentOutputId = ContentOutputId {
    id: 2,
    generation: 1,
};
const ALLOCATION: ContentAllocationId = ContentAllocationId {
    id: 1,
    generation: 1,
};

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    }
}

fn resource(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}

/// What the client saw, reported in order.
#[derive(Debug, Eq, PartialEq)]
enum Seen {
    Uploaded(u64),
    Retiring(ContentResourceId),
    Released(ContentResourceId),
    Kept(ContentResourceId),
    Closed,
}

fn next(client: &mut ShellConnection) -> (TransactionId, ShellContentRecord) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(received) = client.poll_content().unwrap() {
            return received;
        }
        assert!(Instant::now() < deadline, "content response timed out");
        std::thread::yield_now();
    }
}

fn candidate(
    permit: &ContentFramePermit,
    allocation: &ContentAllocationResult,
    generation: u64,
    draws: ContentResourceId,
) -> [ShellContentRecord; 3] {
    [
        ShellContentRecord::CandidateBegin(ContentCandidateBegin {
            grant: grant(),
            candidate_generation: generation,
            output: permit.output,
            facts_generation: 3,
            pacing_permit: permit.permit_id,
            interaction_generation: 4,
            surface_count: 1,
            placement_count: 1,
            target_count: 0,
        }),
        ShellContentRecord::CandidateChunk(ContentCandidateChunk {
            grant: grant(),
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
                resource: draws,
                surface_index: 0,
                destination_x_px: 0,
                destination_y_px: 0,
            }],
            targets: vec![],
        }),
        ShellContentRecord::CandidateEnd(ContentCandidateEnd {
            grant: grant(),
            candidate_generation: generation,
            surface_count: 1,
            placement_count: 1,
            target_count: 0,
        }),
    ]
}

/// The client: frames 1 to 5. Frame g draws resource g (frame 5 redraws 4)
/// and, from frame 2, names the resource the previous frame drew.
///
/// With `quiet`, it runs frames 1 to 4 and makes no further client call
/// between a frame's `Presented` and the host's go-ahead to take its
/// release. Bytes the client queued before that barrier are not traced.
fn client(socket: std::path::PathBuf, seen: mpsc::Sender<Seen>, quiet: Option<mpsc::Receiver<()>>) {
    let options = ShellClientOptions {
        minimum_revision: 12,
        maximum_revision: 12,
        required_capabilities: CONTENT | OWNED | W2,
        handshake_timeout: Duration::from_secs(2),
    };
    let mut client = sdk_image::authenticated_client(&socket, options).unwrap();
    assert!(client.retire_on_present());
    let (_, ShellContentRecord::Limits(limits)) = next(&mut client) else {
        panic!("expected limits");
    };
    let (_, ShellContentRecord::OutputFacts(facts)) = next(&mut client) else {
        panic!("expected output facts");
    };
    assert_eq!(facts.outputs[0].output, OUTPUT);
    client
        .send_content(
            TransactionId::from_raw(6),
            &ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                grant: grant(),
                output: OUTPUT,
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
            }),
        )
        .unwrap();
    let (_, ShellContentRecord::AllocationResult(allocation)) = next(&mut client) else {
        panic!("expected allocation result");
    };
    sdk_image::enable(&mut client, socket.parent().unwrap(), &limits);
    let mut tracker = RetireOnPresent::new(grant());
    let last = if quiet.is_some() { 4 } else { 5 };
    for generation in 1..=last {
        let draws = resource(generation.min(4));
        if generation <= 4 {
            sdk_image::upload(
                &mut client,
                sophia_shell_client::RingUpload {
                    resource: draws.id,
                    resource_generation: draws.generation,
                    output: OUTPUT.id,
                    output_generation: OUTPUT.generation,
                    allocation: allocation.allocation.id,
                    allocation_generation: allocation.allocation.generation,
                    topology_generation: facts.facts_generation,
                    width: 2,
                    height: 1,
                },
                &[0, 0, 255, 255, 0, 128, 0, 128],
            );
            seen.send(Seen::Uploaded(generation)).unwrap();
        }
        client
            .send_content(
                TransactionId::from_raw(9),
                &ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant: grant(),
                    output: OUTPUT,
                    allocation: ContentAllocationId::default(),
                    demand_id: generation,
                    reason: 1,
                }),
            )
            .unwrap();
        let (_, ShellContentRecord::FramePermit(permit)) = next(&mut client) else {
            panic!("expected frame permit");
        };
        assert_eq!(permit.state, 1);
        let records = candidate(&permit, &allocation, generation, draws);
        let transaction = TransactionId::from_raw(20 + generation);
        // Frame 5 names resource 3 again: frame 4's rejection kept it.
        let named = match generation {
            1 => None,
            5 => Some(resource(3)),
            _ => Some(resource(generation - 1)),
        };
        let intent = named.map(|resource| ContentCandidateRetire {
            retire_transaction: TransactionId::from_raw(40 + generation),
            resource,
        });
        match intent {
            None => client.enqueue_content_group(transaction, &records).unwrap(),
            Some(intent) => {
                client
                    .enqueue_content_group_with_retire(transaction, &records, intent)
                    .unwrap();
                tracker.name(OUTPUT, generation, intent).unwrap();
            }
        }
        // Prepared, then the terminal outcome.
        let progress = loop {
            let (tx, record) = next(&mut client);
            let ShellContentRecord::CandidateOutcome(outcome) = &record else {
                panic!("unexpected record {record:?}");
            };
            let progress = tracker.observe(tx, &record).unwrap();
            if outcome.kind != 1 {
                break progress;
            }
        };
        let Some(intent) = intent else {
            assert_eq!(progress, RetireProgress::Unrelated);
            continue;
        };
        match progress {
            RetireProgress::Kept(kept) => {
                assert!(!tracker.holds(kept));
                seen.send(Seen::Kept(kept)).unwrap();
                continue;
            }
            RetireProgress::Retiring(retiring) => {
                assert_eq!(retiring, intent.resource);
                seen.send(Seen::Retiring(retiring)).unwrap();
            }
            other => panic!("unexpected progress {other:?}"),
        }
        if generation == 5 {
            // Held past the end: the owner disconnects before any release.
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match client.poll_content() {
                    Err(_) => break,
                    Ok(Some((_, record))) => panic!("record after the last frame: {record:?}"),
                    Ok(None) => {}
                }
                assert!(Instant::now() < deadline, "owner never disconnected");
                std::thread::yield_now();
            }
            tracker.revoke();
            seen.send(Seen::Closed).unwrap();
            return;
        }
        if let Some(quiet) = &quiet {
            quiet.recv().unwrap();
        }
        // Output facts the host published meanwhile come first.
        let (tx, record) = loop {
            match next(&mut client) {
                (_, ShellContentRecord::OutputFacts(_)) if quiet.is_some() => {}
                received => break received,
            }
        };
        assert!(
            matches!(record, ShellContentRecord::ResourceReleased(_)),
            "{record:?}"
        );
        assert_eq!(
            tracker.observe(tx, &record).unwrap(),
            RetireProgress::Released(intent.resource)
        );
        seen.send(Seen::Released(intent.resource)).unwrap();
    }
    // Stay connected until the host is done.
    if let Some(quiet) = &quiet {
        let _ = quiet.recv();
    }
}

struct Host {
    session: ShellComponentTransport,
    registry: ContentEpochRegistry,
    image_owner: ShellIsbImageOwner,
    seen: mpsc::Receiver<Seen>,
    start: Instant,
    clock: u64,
    allocations: Vec<ContentAllocationSnapshot>,
}

impl Host {
    fn pump(&mut self) {
        let now = self.start.elapsed().as_millis() as u64;
        self.image_owner
            .service(&mut self.session, &mut self.registry, now, Instant::now())
            .unwrap();
        self.session
            .service_content_resources(&mut self.registry, now)
            .unwrap();
        self.session.poll_io(&mut self.registry).unwrap();
        assert!(self.start.elapsed() < Duration::from_secs(10), "stalled");
        std::thread::yield_now();
    }

    fn expect(&mut self, wanted: Seen) {
        loop {
            match self.seen.try_recv() {
                Ok(seen) => {
                    assert_eq!(seen, wanted);
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => self.pump(),
                Err(error) => panic!("client ended early: {error:?}"),
            }
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn retiring_bytes(&self) -> u64 {
        self.session
            .content_accounting(&self.registry)
            .epochs
            .memory
            .retiring
    }

    fn backing_bytes(&self) -> u64 {
        self.session
            .content_accounting(&self.registry)
            .epochs
            .memory
            .backing
    }

    fn resources(&self) -> usize {
        self.session
            .content_accounting(&self.registry)
            .epochs
            .resources
    }

    /// Demand, permit and candidate through Prepared; the render lease
    /// holds every resource the candidate draws.
    fn prepare(&mut self, generation: u64) -> ContentRenderBundle {
        while self.session.next_content_demand(&self.registry).is_none() {
            self.session
                .service_content_demands(&mut self.registry, &[OUTPUT], &self.allocations)
                .unwrap();
            self.pump();
        }
        let now = self.tick();
        self.session
            .grant_content_demand(
                &mut self.registry,
                TransactionId::from_raw(100 + generation),
                OUTPUT,
                generation,
                now,
            )
            .unwrap();
        let allocations = self.allocations.clone();
        let context = ContentCandidateContext {
            output: OUTPUT,
            facts_generation: 3,
            interaction_generation: 4,
            allocations: &allocations,
        };
        let mut processed = 0;
        while processed < 3 {
            let now = self.tick();
            processed += self
                .session
                .service_content_candidates(&mut self.registry, &[context], now)
                .unwrap();
            self.pump();
        }
        let now = self.tick();
        let render = self
            .session
            .begin_content_submission(&mut self.registry, OUTPUT, generation, now)
            .unwrap();
        let now = self.tick();
        self.session
            .content_prepared(&mut self.registry, grant(), OUTPUT, generation, 7, 8, now)
            .unwrap();
        render
    }

    /// One owner pass's wire and image turn (`turn_connected`), without the
    /// bar's content service.
    fn turn(&mut self) {
        let now = self.start.elapsed().as_millis() as u64;
        self.session.poll_io(&mut self.registry).unwrap();
        self.image_owner
            .service(&mut self.session, &mut self.registry, now, Instant::now())
            .unwrap();
    }

    /// One owner pass for a connected bar: the turn, then the production
    /// resource service its `service_content` runs.
    fn pass(&mut self) {
        self.turn();
        let now = self.start.elapsed().as_millis() as u64;
        self.session
            .service_content_resources(&mut self.registry, now)
            .unwrap();
    }

    /// This component's cap on the next owner wait, as
    /// `ShellComponentConnections::authentication_wait` and `output_pending`
    /// contribute it (`authority_receive.rs:58-64`); the owner's own
    /// planner adds the 25 ms maintenance default.
    fn wait_cap(&self) -> Duration {
        let now = Instant::now();
        if self.session.output_pending() {
            return Duration::from_millis(1);
        }
        let images = self
            .image_owner
            .deadline()
            .map_or(Duration::MAX, |at| at.saturating_duration_since(now));
        self.session.authentication_wait(now).min(images)
    }

    fn present(&mut self, generation: u64) {
        self.session
            .content_presented(
                &mut self.registry,
                grant(),
                OUTPUT,
                generation,
                9 + generation,
                7,
                8,
            )
            .unwrap();
    }
}

/// A negotiated r12 session with output facts and one granted allocation.
fn start(quiet: Option<mpsc::Receiver<()>>) -> (Host, std::thread::JoinHandle<()>) {
    start_peer(move |socket, seen| {
        (
            std::thread::spawn(move || client(socket, seen, quiet)),
            std::process::id(),
        )
    })
}
fn start_peer(
    peer: impl FnOnce(std::path::PathBuf, mpsc::Sender<Seen>) -> (std::thread::JoinHandle<()>, u32),
) -> (Host, std::thread::JoinHandle<()>) {
    let directory = std::env::temp_dir().join(format!(
        "sophia-w2-session-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let mut session = ShellComponentTransport::bind_for_supervised_uid(
        directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    session
        .reserve_content(&mut registry, ContentLimits::prototype(grant()))
        .unwrap();
    let socket = session.socket_path().to_path_buf();
    let (seen_tx, seen) = mpsc::channel();
    let (client, peer_pid) = peer(socket, seen_tx);
    let image_owner =
        image::configure_for_peer(&mut session, &ContentLimits::prototype(grant()), peer_pid);
    session.permit_retire_on_present(true).unwrap();

    session
        .begin_file_negotiation(
            &registry,
            1,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let welcome = loop {
        if let Some(welcome) = session.poll_negotiation(&mut registry, 65536).unwrap() {
            break welcome;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    assert_eq!(welcome.selected_revision, 12);
    assert_ne!(welcome.capabilities & W2, 0);
    let mut host = Host {
        session,
        registry,
        image_owner,
        seen,
        start: Instant::now(),
        clock: 10,
        allocations: Vec::new(),
    };
    host.session
        .publish_content_output_facts(
            &mut host.registry,
            TransactionId::from_raw(5),
            3,
            vec![ContentOutputFactsEntry {
                output: OUTPUT,
                local_width: 64,
                local_height: 64,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 5,
            }],
        )
        .unwrap();
    while host
        .session
        .next_content_allocation_request(&host.registry)
        .is_none()
    {
        let now = host.start.elapsed().as_millis() as u64;
        host.session
            .service_content_allocation_requests(&mut host.registry, &[], now)
            .unwrap();
        host.pump();
    }
    let snapshot = ContentAllocationSnapshot {
        native_opening: None,
        output: OUTPUT,
        allocation: ALLOCATION,
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
    };
    host.session
        .grant_content_allocation(&mut host.registry, 1, snapshot, &[])
        .unwrap();
    host.allocations = host.session.content_allocation_snapshots(&host.registry);
    (host, client)
}

#[test]
fn successors_retire_predecessors_only_after_their_last_lease() {
    retirement_lifecycle(start(None));
}

#[test]
fn c_successors_retire_predecessors_only_after_their_last_lease() {
    let binary = c_peer::compile();
    retirement_lifecycle(start_peer(move |socket, seen| binary.spawn(&socket, seen)));
}

fn retirement_lifecycle((mut host, client): (Host, std::thread::JoinHandle<()>)) {
    // Frame 1 names nothing; its render lease is kept.
    host.expect(Seen::Uploaded(1));
    let render1 = host.prepare(1);
    host.present(1);

    // Frame 2 names resource 1 while frame 1's lease still holds it: the
    // retire is committed at Presented, but no release may follow yet.
    host.expect(Seen::Uploaded(2));
    let render2 = host.prepare(2);
    host.present(2);
    host.expect(Seen::Retiring(resource(1)));
    let held = host.resources();
    for _ in 0..50 {
        host.pump();
    }
    assert!(host.retiring_bytes() > 0, "the committed retire is charged");
    assert!(matches!(
        host.seen.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    drop(render1);
    host.expect(Seen::Released(resource(1)));
    assert_eq!(host.retiring_bytes(), 0);
    assert_eq!(host.resources(), held - 1, "the released resource is gone");

    // Frame 3: frame 2's lease ends before presentation, so the release
    // follows the commit directly.
    host.expect(Seen::Uploaded(3));
    let render3 = host.prepare(3);
    drop(render2);
    host.present(3);
    host.expect(Seen::Retiring(resource(2)));
    host.expect(Seen::Released(resource(2)));
    assert_eq!(host.retiring_bytes(), 0);

    // Frame 4 names resource 3 but fails to render: resource 3 stays the
    // client's, uncharged and accepted.
    host.expect(Seen::Uploaded(4));
    let resources = host.resources();
    let render4 = host.prepare(4);
    host.session
        .content_renderer_failed(&mut host.registry, grant(), OUTPUT, 4)
        .unwrap();
    host.expect(Seen::Kept(resource(3)));
    assert_eq!(host.retiring_bytes(), 0);
    assert_eq!(host.resources(), resources);

    // Frame 5 names resource 3 again, still leased by frame 3's render, and
    // the owner disconnects while it is retiring.
    let render5 = host.prepare(5);
    host.present(5);
    host.expect(Seen::Retiring(resource(3)));
    assert!(host.retiring_bytes() > 0);
    host.session.disconnect(&mut host.registry).unwrap();
    host.image_owner
        .service(&mut host.session, &mut host.registry, 0, Instant::now())
        .unwrap();
    assert_eq!(
        host.seen.recv_timeout(Duration::from_secs(5)),
        Ok(Seen::Closed)
    );
    client.join().unwrap();
    // Collection cannot release a live consumer; the last drop can.
    host.registry.collect();
    assert!(!host.session.content_accounting(&host.registry).quiescent());
    drop((render3, render4, render5));
    host.session.disconnect(&mut host.registry).unwrap();
    host.registry.collect();
    assert!(
        host.session
            .collect_content_accounting(&mut host.registry)
            .quiescent()
    );
}

/// The owner planner's idle default (`owner_loop.rs` `authority_wait_timeout`).
const MAINTENANCE: Duration = Duration::from_millis(25);

/// A composed proof, not a full owner-loop wake test: this component's wait
/// inputs under the production calls, with each owner pass replayed
/// explicitly and no client call after each barrier. Source wiring (not
/// exercised here):
/// every owner pass with components runs `service_components` →
/// `bar.service_content` → `service_content_resources`.
#[test]
fn a_dropped_lease_is_released_by_the_next_resource_service_and_nothing_sooner() {
    let (go, quiet) = mpsc::channel();
    let (mut host, client) = start(Some(quiet));
    host.expect(Seen::Uploaded(1));
    let render1 = host.prepare(1);
    host.present(1);
    // Control, W2 negotiated with no retire intent pending (not a W2-off
    // control): nothing retiring and nothing pending caps no wait.
    host.expect(Seen::Uploaded(2));
    for _ in 0..3 {
        host.pass();
        assert!(host.wait_cap() >= MAINTENANCE, "{:?}", host.wait_cap());
    }
    assert_eq!(host.retiring_bytes(), 0);

    // Frame 2 commits resource 1's retire while frame 1's render holds it.
    let render2 = host.prepare(2);
    host.present(2);
    host.expect(Seen::Retiring(resource(1)));
    let resources = host.resources();
    let backing = host.backing_bytes();
    // Held across several maintenance passes: charged, unreleased, and no
    // shorter wait than maintenance (no busy loop).
    for _ in 0..5 {
        host.pass();
        assert!(host.retiring_bytes() > 0);
        assert!(!host.session.output_pending());
        assert!(host.wait_cap() >= MAINTENANCE, "{:?}", host.wait_cap());
    }

    // Dropped just after a pass's resource service: no notification, so the
    // wait inputs are unchanged and a turn alone collects nothing.
    drop(render1);
    assert!(host.wait_cap() >= MAINTENANCE);
    host.turn();
    assert!(
        host.retiring_bytes() > 0,
        "only the resource service collects"
    );
    assert!(host.wait_cap() >= MAINTENANCE);
    // The next pass's resource service collects the release and, with
    // journal room, appends it in the same call: the 8-byte backing is freed
    // only once the record is in the live journal (`confirm_isb_release`).
    host.pass();
    assert_eq!(host.retiring_bytes(), 0);
    assert_eq!(host.resources(), resources - 1);
    assert_eq!(host.backing_bytes(), backing - 8, "journalled, then freed");
    assert!(!host.session.output_pending());
    assert!(host.wait_cap() >= MAINTENANCE);
    go.send(()).unwrap();
    host.expect(Seen::Released(resource(1)));

    // Frame 3 names resource 2, held by frame 2's render. Dropped just
    // before a pass's resource service, it is released in that same pass.
    host.expect(Seen::Uploaded(3));
    let render3 = host.prepare(3);
    host.present(3);
    host.expect(Seen::Retiring(resource(2)));
    host.pass();
    assert!(host.retiring_bytes() > 0);
    host.turn();
    drop(render2);
    let now = host.start.elapsed().as_millis() as u64;
    host.session
        .service_content_resources(&mut host.registry, now)
        .unwrap();
    assert_eq!(host.retiring_bytes(), 0);
    assert!(!host.session.output_pending());
    assert!(host.wait_cap() >= MAINTENANCE);
    go.send(()).unwrap();
    host.expect(Seen::Released(resource(2)));

    // Frame 4 names resource 3, held by frame 3's render. Output facts the
    // silent client does not read fill the journal until a publish is
    // refused as saturated.
    host.expect(Seen::Uploaded(4));
    let render4 = host.prepare(4);
    host.present(4);
    host.expect(Seen::Retiring(resource(3)));
    let mut facts = 0;
    loop {
        let published = host.session.publish_content_output_facts(
            &mut host.registry,
            TransactionId::from_raw(200 + facts),
            4 + facts,
            vec![ContentOutputFactsEntry {
                output: OUTPUT,
                local_width: 64,
                local_height: 64,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 5,
            }],
        );
        host.turn();
        match published {
            Ok(()) => facts += 1,
            Err(ShellTransportError::ContentQueueSaturated) => break,
            Err(error) => panic!("facts: {error:?}"),
        }
        assert!(facts < 1024, "the journal never filled");
    }
    assert!(facts > 0);
    assert!(!host.session.output_pending());
    assert!(host.wait_cap() >= MAINTENANCE);
    // Capacity-blocked: with the lease dropped custody permits the release,
    // but it keeps its charge until the record enters the journal. Nothing
    // caps the wait meanwhile; the client's next read and ACK wake the owner.
    let backing = host.backing_bytes();
    drop(render3);
    for _ in 0..5 {
        host.pass();
        assert_eq!(host.retiring_bytes(), 8, "no release claim while blocked");
        assert_eq!(host.backing_bytes(), backing);
        assert!(!host.session.output_pending());
        assert!(host.wait_cap() >= MAINTENANCE, "{:?}", host.wait_cap());
    }
    go.send(()).unwrap();
    host.expect(Seen::Released(resource(3)));
    assert_eq!(host.retiring_bytes(), 0);
    assert_eq!(host.backing_bytes(), backing - 8);

    drop(go);
    client.join().unwrap();
    drop(render4);
    host.session.disconnect(&mut host.registry).unwrap();
    host.registry.collect();
}
