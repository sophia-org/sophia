//! W2 prototype over the real wire: what ends an uncommitted retire intent
//! before Presented, and what an owner revocation leaves behind. The real SDK
//! client negotiates r12 with bit 17 and acts only on the host's commands, so
//! the host orders every step; candidate time is the host's clock, so expiry
//! needs no sleep. Renderer completions and authentication are supplied.
//!
//! Every revocation of a component's content grant is
//! `ShellComponentTransport::disconnect`; "non-disconnect" here means the
//! owner revokes while the peer is still attached and has not closed.
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
/// Every intent here names resource 1 on this retire ID.
const RETIRE: ContentCandidateRetire = ContentCandidateRetire {
    retire_transaction: TransactionId::from_raw(42),
    resource: ContentResourceId {
        id: 1,
        generation: 1,
    },
};

fn resource(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}

fn grant(epoch: u64) -> ContentGrant {
    ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    }
}

/// What the host asks of the client.
enum Cmd {
    Upload(u64),
    Demand(u64),
    /// A candidate under the last permit read, drawing one resource.
    Candidate {
        generation: u64,
        draws: u64,
        retire: Option<ContentCandidateRetire>,
    },
    /// The next session record.
    Read,
    /// Every record until the session ends the connection.
    UntilClosed,
}

enum Reply {
    Done,
    Record(TransactionId, Box<ShellContentRecord>),
    Closed(Vec<ShellContentRecord>),
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
    grant: ContentGrant,
    permit: &ContentFramePermit,
    allocation: &ContentAllocationResult,
    generation: u64,
    draws: ContentResourceId,
) -> [ShellContentRecord; 3] {
    [
        ShellContentRecord::CandidateBegin(ContentCandidateBegin {
            grant,
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
                resource: draws,
                surface_index: 0,
                destination_x_px: 0,
                destination_y_px: 0,
            }],
            targets: vec![],
        }),
        ShellContentRecord::CandidateEnd(ContentCandidateEnd {
            grant,
            candidate_generation: generation,
            surface_count: 1,
            placement_count: 1,
            target_count: 0,
        }),
    ]
}

/// The client: setup, then one command at a time. Between commands it only
/// drives its own I/O, so what it sent reaches the session.
fn client(socket: std::path::PathBuf, cmds: mpsc::Receiver<Cmd>, replies: mpsc::Sender<Reply>) {
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
    client
        .send_content(
            TransactionId::from_raw(6),
            &ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                grant: limits.grant,
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
    let mut permit = None;
    let mut open = true;
    loop {
        let cmd = match cmds.try_recv() {
            Ok(cmd) => cmd,
            Err(mpsc::TryRecvError::Disconnected) => return,
            Err(mpsc::TryRecvError::Empty) => {
                // A revoked connection ends here; only UntilClosed reports it.
                open = open && client.poll_io().is_ok();
                std::thread::yield_now();
                continue;
            }
        };
        let reply = match cmd {
            Cmd::Upload(id) => {
                sdk_image::upload(
                    &mut client,
                    sophia_shell_client::RingUpload {
                        resource: id,
                        resource_generation: 1,
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
                Reply::Done
            }
            Cmd::Demand(id) => {
                client
                    .send_content(
                        TransactionId::from_raw(9),
                        &ShellContentRecord::FrameDemand(ContentFrameDemand {
                            grant: limits.grant,
                            output: OUTPUT,
                            allocation: ContentAllocationId::default(),
                            demand_id: id,
                            reason: 1,
                        }),
                    )
                    .unwrap();
                Reply::Done
            }
            Cmd::Candidate {
                generation,
                draws,
                retire,
            } => {
                let records = candidate(
                    limits.grant,
                    permit.as_ref().expect("a permit was read"),
                    &allocation,
                    generation,
                    resource(draws),
                );
                let transaction = TransactionId::from_raw(20 + generation);
                match retire {
                    None => client.enqueue_content_group(transaction, &records),
                    Some(retire) => {
                        client.enqueue_content_group_with_retire(transaction, &records, retire)
                    }
                }
                .unwrap();
                Reply::Done
            }
            Cmd::Read => {
                let (transaction, record) = next(&mut client);
                if let ShellContentRecord::FramePermit(value) = &record {
                    permit = Some(value.clone());
                }
                Reply::Record(transaction, Box::new(record))
            }
            Cmd::UntilClosed => {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut seen = Vec::new();
                loop {
                    match client.poll_content() {
                        Err(_) => break,
                        Ok(Some((_, record))) => seen.push(record),
                        Ok(None) => {}
                    }
                    assert!(Instant::now() < deadline, "owner never closed");
                    std::thread::yield_now();
                }
                Reply::Closed(seen)
            }
        };
        if replies.send(reply).is_err() {
            return;
        }
    }
}

struct Peer {
    cmds: mpsc::Sender<Cmd>,
    replies: mpsc::Receiver<Reply>,
    thread: std::thread::JoinHandle<()>,
    tracker: RetireOnPresent,
}

struct Host {
    session: ShellComponentTransport,
    registry: ContentEpochRegistry,
    image_owner: Option<ShellIsbImageOwner>,
    start: Instant,
    clock: u64,
    allocations: Vec<ContentAllocationSnapshot>,
    grant: ContentGrant,
    peer: Option<Peer>,
}

impl Host {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sophia-w2-lifecycle-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Self {
            session: ShellComponentTransport::bind_for_supervised_uid(
                directory,
                rustix::process::geteuid().as_raw(),
            )
            .unwrap(),
            registry: ContentEpochRegistry::new(64 * 1024 * 1024).unwrap(),
            image_owner: None,
            start: Instant::now(),
            clock: 10,
            allocations: Vec::new(),
            grant: grant(0),
            peer: None,
        }
    }

    /// A negotiated r12 connection under `grant(epoch)`, with output facts,
    /// one granted allocation and resources 1 and 2 uploaded.
    fn connect(&mut self, epoch: u64) {
        self.grant = grant(epoch);
        let limits = ContentLimits::prototype(self.grant);
        self.session
            .reserve_content(&mut self.registry, limits.clone())
            .unwrap();
        self.image_owner = Some(image::configure(&mut self.session, &limits));
        self.session.permit_retire_on_present(true).unwrap();
        let socket = self.session.socket_path().to_path_buf();
        let (cmds, cmd_rx) = mpsc::channel();
        let (reply_tx, replies) = mpsc::channel();
        let thread = std::thread::spawn(move || client(socket, cmd_rx, reply_tx));
        self.peer = Some(Peer {
            cmds,
            replies,
            thread,
            tracker: RetireOnPresent::new(self.grant),
        });
        self.session
            .begin_file_negotiation(
                &self.registry,
                epoch,
                Duration::from_secs(2),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: false,
                },
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let welcome = loop {
            if let Some(welcome) = self
                .session
                .poll_negotiation(&mut self.registry, 65536)
                .unwrap()
            {
                break welcome;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(welcome.selected_revision, 12);
        assert_ne!(welcome.capabilities & W2, 0);
        self.session
            .publish_content_output_facts(
                &mut self.registry,
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
        while self
            .session
            .next_content_allocation_request(&self.registry)
            .is_none()
        {
            let now = self.start.elapsed().as_millis() as u64;
            self.session
                .service_content_allocation_requests(&mut self.registry, &[], now)
                .unwrap();
            self.pump();
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
        self.session
            .grant_content_allocation(&mut self.registry, 1, snapshot, &[])
            .unwrap();
        self.allocations = self.session.content_allocation_snapshots(&self.registry);
        self.call(Cmd::Upload(1));
        self.call(Cmd::Upload(2));
    }

    /// One owner pass while connected; after a revocation, collection only.
    fn pump(&mut self) {
        if self.session.content_limits().is_some() {
            let now = self.start.elapsed().as_millis() as u64;
            if let Some(owner) = &mut self.image_owner {
                owner
                    .service(&mut self.session, &mut self.registry, now, Instant::now())
                    .unwrap();
            }
            self.session
                .service_content_resources(&mut self.registry, now)
                .unwrap();
            self.session.poll_io(&mut self.registry).unwrap();
        } else {
            self.registry.collect();
        }
        assert!(self.start.elapsed() < Duration::from_secs(10), "stalled");
        std::thread::yield_now();
    }

    fn call(&mut self, cmd: Cmd) -> Reply {
        self.peer.as_ref().unwrap().cmds.send(cmd).unwrap();
        loop {
            match self.peer.as_ref().unwrap().replies.try_recv() {
                Ok(reply) => return reply,
                Err(mpsc::TryRecvError::Empty) => self.pump(),
                Err(error) => panic!("client ended early: {error:?}"),
            }
        }
    }

    /// The client's next record and what its reducer made of it.
    fn read(&mut self) -> (TransactionId, ShellContentRecord, RetireProgress) {
        let Reply::Record(transaction, record) = self.call(Cmd::Read) else {
            panic!("expected a record");
        };
        let record = *record;
        let progress = self
            .peer
            .as_mut()
            .unwrap()
            .tracker
            .observe(transaction, &record)
            .unwrap();
        (transaction, record, progress)
    }

    /// The next record must be `generation`'s outcome of `kind`.
    fn outcome(&mut self, generation: u64, kind: u16) -> RetireProgress {
        let (_, record, progress) = self.read();
        let ShellContentRecord::CandidateOutcome(outcome) = &record else {
            panic!("expected an outcome, got {record:?}");
        };
        assert_eq!(
            (outcome.candidate_generation, outcome.kind),
            (generation, kind),
            "{outcome:?}"
        );
        progress
    }

    fn permit_read(&mut self) {
        let (_, record, _) = self.read();
        assert!(
            matches!(&record, ShellContentRecord::FramePermit(p) if p.state == 1),
            "{record:?}"
        );
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// The client demands frame `generation`; the host grants it.
    fn demand(&mut self, generation: u64) {
        self.call(Cmd::Demand(generation));
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
    }

    /// The client sends the candidate; the host admits it as pending.
    fn admit(&mut self, generation: u64, draws: u64, retire: Option<ContentCandidateRetire>) {
        self.call(Cmd::Candidate {
            generation,
            draws,
            retire,
        });
        if let Some(retire) = retire {
            let tracker = &mut self.peer.as_mut().unwrap().tracker;
            tracker.name(OUTPUT, generation, retire).unwrap();
        }
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
    }

    fn submit(&mut self, generation: u64) -> ContentRenderBundle {
        let now = self.tick();
        self.session
            .begin_content_submission(&mut self.registry, OUTPUT, generation, now)
            .unwrap()
    }

    fn prepared(&mut self, generation: u64) {
        let now = self.tick();
        let grant = self.grant;
        self.session
            .content_prepared(&mut self.registry, grant, OUTPUT, generation, 7, 8, now)
            .unwrap();
    }

    fn presented(
        &mut self,
        grant: ContentGrant,
        generation: u64,
    ) -> Result<(), ShellTransportError> {
        self.session.content_presented(
            &mut self.registry,
            grant,
            OUTPUT,
            generation,
            9 + generation,
            7,
            8,
        )
    }

    /// Frame `generation` through Presented; its render lease is returned.
    fn frame(
        &mut self,
        generation: u64,
        draws: u64,
        retire: Option<ContentCandidateRetire>,
    ) -> (ContentRenderBundle, RetireProgress) {
        self.demand(generation);
        self.permit_read();
        self.admit(generation, draws, retire);
        let render = self.submit(generation);
        self.prepared(generation);
        self.outcome(generation, 1);
        self.presented(self.grant, generation).unwrap();
        let progress = self.outcome(generation, 2);
        (render, progress)
    }

    fn store(&self) -> &ContentResourceStore {
        self.registry.resources(self.grant).unwrap()
    }

    fn reserved(&self) -> u64 {
        self.store().reserved_retiring()
    }

    fn memory(&self) -> ContentMemoryUsage {
        self.registry.accounting().memory
    }

    /// Revocation as `ShellComponentConnections` performs it: disconnect,
    /// then one image-owner service.
    fn disconnect(&mut self) {
        self.session.disconnect(&mut self.registry).unwrap();
        let now = self.start.elapsed().as_millis() as u64;
        if let Some(owner) = &mut self.image_owner {
            owner
                .service(&mut self.session, &mut self.registry, now, Instant::now())
                .unwrap();
        }
    }

    /// The owner revokes this grant while the client is still attached.
    fn revoke(&mut self) -> Vec<ShellContentRecord> {
        self.disconnect();
        let Reply::Closed(seen) = self.call(Cmd::UntilClosed) else {
            panic!("expected the close");
        };
        let peer = self.peer.take().unwrap();
        drop(peer.cmds);
        peer.thread.join().unwrap();
        seen
    }

    fn finish(mut self) {
        if let Some(peer) = self.peer.take() {
            self.disconnect();
            drop(peer.cmds);
            peer.thread.join().unwrap();
        }
        self.registry.collect();
        let accounting = self.session.collect_content_accounting(&mut self.registry);
        assert!(accounting.quiescent(), "{accounting:?}");
    }
}

/// Frame 1 draws resource 1 and stays leased; frame 2 draws 2 and names 1.
fn named(host: &mut Host) -> ContentRenderBundle {
    host.connect(1);
    let (render1, progress) = host.frame(1, 1, None);
    assert_eq!(progress, RetireProgress::Unrelated);
    host.demand(2);
    host.permit_read();
    host.admit(2, 2, Some(RETIRE));
    assert_eq!(host.reserved(), 8, "admission reserves the predecessor");
    render1
}

#[test]
fn supersession_and_expiry_free_the_intent_before_the_next_admission() {
    let mut host = Host::new();
    let render1 = named(&mut host);

    // Supersession: a new permit for the output replaces pending frame 2.
    host.demand(3);
    assert_eq!(
        host.outcome(2, 4),
        RetireProgress::Kept(RETIRE.resource),
        "Superseded"
    );
    assert_eq!(host.reserved(), 0);
    host.permit_read();
    // The same predecessor on the same retire ID is admitted at once.
    host.admit(3, 2, Some(RETIRE));
    assert_eq!(host.reserved(), 8);

    // Expiry under the host's candidate clock: one millisecond before the
    // preparation deadline frame 3 still holds; at the deadline it is gone.
    let timeout = u64::from(
        host.session
            .content_limits()
            .unwrap()
            .preparation_timeout_ms,
    );
    let allocations = host.allocations.clone();
    let context = ContentCandidateContext {
        output: OUTPUT,
        facts_generation: 3,
        interaction_generation: 4,
        allocations: &allocations,
    };
    let admitted = host.clock;
    for (now, held) in [(admitted + timeout - 1, 8), (admitted + timeout, 0)] {
        host.clock = now;
        host.session
            .service_content_candidates(&mut host.registry, &[context], now)
            .unwrap();
        assert_eq!(host.reserved(), held, "at {now}");
    }
    assert_eq!(host.outcome(3, 3), RetireProgress::Kept(RETIRE.resource));
    assert_eq!(host.memory().retiring, 0);

    // Frame 4 names it a third time and is presented; the release waits for
    // frame 1's lease and arrives on the same retire ID.
    let (render4, progress) = host.frame(4, 2, Some(RETIRE));
    assert_eq!(progress, RetireProgress::Retiring(RETIRE.resource));
    assert_eq!(host.memory().retiring, 8);
    for _ in 0..5 {
        host.pump();
    }
    assert_eq!(host.memory().retiring, 8, "frame 1 still leases it");
    drop(render1);
    let (transaction, record, progress) = host.read();
    assert!(matches!(record, ShellContentRecord::ResourceReleased(_)));
    assert_eq!(transaction, RETIRE.retire_transaction);
    assert_eq!(progress, RetireProgress::Released(RETIRE.resource));
    drop(render4);
    host.finish();
}

const STALE: ShellTransportError =
    ShellTransportError::ContentCandidate(ContentCandidateError::Stale);

#[test]
fn owner_revocation_cancels_an_uncommitted_intent_and_waits_for_the_last_lease() {
    // Frame 2 is pending, submitted, then Prepared when the owner revokes.
    for stage in 0..3 {
        let mut host = Host::new();
        let render1 = named(&mut host);
        let render2 = (stage > 0).then(|| host.submit(2));
        if stage == 2 {
            host.prepared(2);
            assert_eq!(host.outcome(2, 1), RetireProgress::Unrelated);
        }
        let old = host.grant;
        // The attached client is sent nothing more: no outcome, no release.
        assert_eq!(host.revoke(), [], "stage {stage}");
        assert_eq!(host.memory().retiring, 0, "stage {stage}: never committed");
        // A late completion settles the retained candidate without committing
        // it; its replay is stale.
        let late = |host: &mut Host| match stage {
            1 => host
                .session
                .content_renderer_failed(&mut host.registry, old, OUTPUT, 2),
            _ => host.presented(old, 2),
        };
        if stage > 0 {
            assert_eq!(late(&mut host), Ok(()), "stage {stage}");
            assert_eq!(late(&mut host), Err(STALE), "stage {stage}");
        }
        assert_eq!(host.memory().retiring, 0, "stage {stage}");
        // Frame 1's lease keeps resource 1, and frame 2's keeps resource 2,
        // across collects; each goes only with its last lease.
        let held = if stage == 0 { 8 } else { 16 };
        for _ in 0..3 {
            host.registry.collect();
            assert_eq!(host.memory().backing, held, "stage {stage}");
        }
        drop(render2);
        host.registry.collect();
        assert_eq!(host.memory().backing, 8, "stage {stage}: frame 1 holds 1");
        drop(render1);
        host.finish();
    }
}

#[test]
fn a_late_completion_or_replay_neither_commits_nor_touches_the_replacement_grant() {
    let mut host = Host::new();
    let render1 = named(&mut host);
    let render2 = host.submit(2);
    host.prepared(2);
    assert_eq!(host.outcome(2, 1), RetireProgress::Unrelated);
    let old = host.grant;
    assert_eq!(host.revoke(), []);

    // The replacement grant on the same transport reuses every identity:
    // resources 1 and 2, and a Prepared frame 2 naming 1 on retire ID 42.
    host.connect(2);
    let (replacement1, _) = host.frame(1, 1, None);
    host.demand(2);
    host.permit_read();
    host.admit(2, 2, Some(RETIRE));
    let replacement2 = host.submit(2);
    host.prepared(2);
    assert_eq!(host.outcome(2, 1), RetireProgress::Unrelated);
    let before = format!("{:?}", host.store().usage());
    assert_eq!(host.reserved(), 8);

    // The revoked frame 2 completes late, then is replayed both ways.
    assert_eq!(host.presented(old, 2), Ok(()));
    assert_eq!(host.presented(old, 2), Err(STALE));
    assert_eq!(
        host.session
            .content_renderer_failed(&mut host.registry, old, OUTPUT, 2),
        Err(STALE)
    );
    assert_eq!(
        host.memory().retiring,
        0,
        "nothing committed in either grant"
    );
    assert_eq!(host.reserved(), 8, "the replacement keeps its own intent");
    assert_eq!(format!("{:?}", host.store().usage()), before);

    // The replacement's own Presented is the next record on its wire.
    host.presented(host.grant, 2).unwrap();
    assert_eq!(
        host.outcome(2, 2),
        RetireProgress::Retiring(RETIRE.resource)
    );
    drop(replacement1);
    let (transaction, record, progress) = host.read();
    assert!(matches!(record, ShellContentRecord::ResourceReleased(_)));
    assert_eq!(transaction, RETIRE.retire_transaction);
    assert_eq!(progress, RetireProgress::Released(RETIRE.resource));

    // The revoked epoch still holds both of its resources until their leases end.
    assert_eq!(host.registry.accounting().retired_epochs, 1);
    drop((render1, render2));
    host.registry.collect();
    assert_eq!(host.registry.accounting().retired_epochs, 0);
    drop(replacement2);
    host.finish();
}
