//! B6c live fixtures: the production shell client
//! (`sophia_shell_client::ShellConnection::connect_files`, its `*_tracked`
//! admissions, `custody` and `wake_deadline`) against the production
//! `ShellComponentTransport` file export, its journal and its owners' intake.
//!
//! Everything runs on one thread in lockstep after the (blocking) connect:
//! a client pass, a transport pass, repeated, so each fixture decides what
//! either side can see. Between them sits a transparent tap
//! (`support/shell_client_b6c_live/tap.rs`) that forwards every byte in
//! order and records what crossed the wire; it is how a fixture proves a
//! `submit` was issued and what the transport answered, rather than
//! trusting local queueing.
//!
//! Session policy stays out of scope. Where the transport's boundary takes
//! a decision from Session (`finish_catalog_activation`,
//! `finish_indicator_activation`), the fixture passes an opaque value and
//! asserts only that the transport carries it back exactly; no candidate is
//! serviced, prepared or presented, and nothing here claims a launch or a
//! presentation.

#[path = "support/shell_client_b6c_live/tap.rs"]
mod tap;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{
    Admission, CatalogInbox, CatalogObservation, ContentLifecycle, Custody, ShellClientError,
    ShellClientOptions, ShellConnection, Ticket,
};
use tap::{Seen, Tap};

const MIB: u64 = 1024 * 1024;
const EPOCH: u64 = 1;
const EAGAIN: u32 = 11;
const EACCES: u32 = 13;
const ESTALE: u32 = 116;
/// Every lockstep wait is bounded by this.
const WAIT: Duration = Duration::from_secs(5);

const BAR: u64 =
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
const INDICATORS: u64 = SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS;
const INDICATOR_ACTIVATION: u64 = SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION;
const DOCK: u64 = SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG
    | SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
    | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn directory(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "shell-client-b6c-live-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn tx(raw: u64) -> TransactionId {
    TransactionId::from_raw(raw)
}

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: EPOCH,
        content_grant_epoch: EPOCH,
    }
}

fn output() -> ContentOutputId {
    ContentOutputId {
        id: 2,
        generation: 1,
    }
}

/// The dock's reduced content limits, as `support/native_launcher_content.rs`
/// grants them.
fn dock_limits() -> ContentLimits {
    let mut limits = ContentLimits::prototype(grant());
    limits.max_staging_bytes = 4 * MIB;
    limits.max_resident_bytes = 12 * MIB;
    limits.max_retiring_bytes = 8 * MIB;
    limits
}

// ---- the live pair ----

#[derive(Clone, Copy)]
enum Role {
    /// The bar (r6) with a content grant and these indicator bits.
    Bar(u64),
    /// The dock (r8): the exact persistent-catalog mask.
    Dock,
}

struct Live {
    registry: ContentEpochRegistry,
    transport: ShellComponentTransport,
    tap: Tap,
    client: ShellConnection,
    directories: [PathBuf; 2],
    revoked: bool,
}

impl Live {
    /// Binds a real transport endpoint, reserves the role's content grant,
    /// and connects the production client to it through the tap: the
    /// client's blocking connect runs on its own thread while this one
    /// drives negotiation, then the connection comes back for lockstep.
    fn connect(role: Role) -> Self {
        let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
        let server_directory = directory("server");
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            &server_directory,
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
        let (options, policy) = match role {
            Role::Bar(bits) => {
                transport
                    .reserve_content(&mut registry, ContentLimits::prototype(grant()))
                    .unwrap();
                (
                    ShellClientOptions {
                        minimum_revision: 5,
                        maximum_revision: 6,
                        required_capabilities: BAR | bits,
                        handshake_timeout: Duration::from_secs(2),
                    },
                    ShellContentAdmissionPolicy::Granted {
                        discrete_input: false,
                    },
                )
            }
            Role::Dock => {
                transport
                    .reserve_content_with_profile(
                        &mut registry,
                        dock_limits(),
                        ContentStoreProfile::PersistentCatalog,
                    )
                    .unwrap();
                (
                    ShellClientOptions {
                        minimum_revision: 8,
                        maximum_revision: 8,
                        required_capabilities: DOCK,
                        handshake_timeout: Duration::from_secs(2),
                    },
                    ShellContentAdmissionPolicy::Granted {
                        discrete_input: true,
                    },
                )
            }
        };
        let tap_directory = directory("tap");
        std::fs::create_dir_all(&tap_directory).unwrap();
        let tap_path = tap_directory.join("shell.sock");
        let mut tap = Tap::bind(&tap_path, transport.socket_path());
        let client = std::thread::spawn(move || ShellConnection::connect_files(&tap_path, options));
        transport
            .begin_file_negotiation(&registry, EPOCH, Duration::from_secs(2), policy)
            .unwrap();
        let deadline = Instant::now() + WAIT;
        let mut negotiated = false;
        while !client.is_finished() {
            tap.pump();
            if negotiated {
                transport.poll_io(&mut registry).unwrap();
            } else if transport
                .poll_negotiation(&mut registry, 64 * 1024)
                .unwrap()
                .is_some()
            {
                negotiated = true;
            }
            tap.pump();
            assert!(Instant::now() < deadline, "the live handshake stalled");
            std::thread::yield_now();
        }
        let client = client.join().unwrap().expect("connect_files");
        assert!(negotiated, "the transport completed negotiation");
        assert_eq!(client.connection_epoch(), EPOCH);
        let mut live = Self {
            registry,
            transport,
            tap,
            client,
            directories: [server_directory, tap_directory],
            revoked: false,
        };
        // The client's first pass after connect puts its `events` read out;
        // the handshake's own traffic is not what fixtures assert on.
        live.turn().unwrap();
        live.tap.seen.clear();
        live
    }

    /// The transport's own pass: its 9P server turn and journal flush.
    fn serve(&mut self) {
        match self.transport.poll_io(&mut self.registry) {
            Ok(()) => {}
            Err(ShellTransportError::NotConnected) if self.revoked => {}
            Err(error) => panic!("transport poll: {error}"),
        }
    }

    /// One lockstep round: transport, then client, each seeing what the tap
    /// delivered.
    fn turn(&mut self) -> Result<(), ShellClientError> {
        self.tap.pump();
        if !self.revoked {
            self.serve();
        }
        self.tap.pump();
        let result = self.client.poll_io();
        self.tap.pump();
        result
    }

    /// Rounds until `done`, within [`WAIT`]; the client must not fail.
    fn until(&mut self, mut done: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + WAIT;
        while !done(self) {
            assert!(Instant::now() < deadline, "the live exchange stalled");
            self.turn().unwrap();
            std::thread::yield_now();
        }
    }

    /// Rounds until `ticket` leaves `Queued`/`InFlight`, tolerating the
    /// client's own failure (a revocation is one).
    fn settle(&mut self, ticket: Ticket) -> Custody {
        let deadline = Instant::now() + WAIT;
        loop {
            let _ = self.turn();
            match self.client.custody(ticket) {
                Some(Custody::Queued | Custody::InFlight) => {}
                Some(custody) => return custody,
                None => panic!("ticket {ticket:?} evicted"),
            }
            assert!(Instant::now() < deadline, "custody never settled");
            std::thread::yield_now();
        }
    }

    fn custody(&self, ticket: Ticket) -> Option<Custody> {
        self.client.custody(ticket)
    }

    /// Rounds until `take` yields a value.
    fn next<T>(
        &mut self,
        mut take: impl FnMut(&mut ShellConnection) -> Result<Option<T>, ShellClientError>,
    ) -> T {
        let deadline = Instant::now() + WAIT;
        loop {
            self.turn().unwrap();
            if let Some(value) = take(&mut self.client).unwrap() {
                return value;
            }
            assert!(Instant::now() < deadline, "nothing was delivered");
            std::thread::yield_now();
        }
    }

    /// The next whole catalog the client delivers, skipping content records
    /// (the dock's `limits`).
    fn next_catalog(
        &mut self,
        inbox: &mut CatalogInbox,
    ) -> (TransactionId, ShellPersistentCatalog) {
        loop {
            match self.next(|client| client.take_catalog_observation(inbox)) {
                CatalogObservation::Catalog(transaction, catalog) => return (transaction, catalog),
                CatalogObservation::Content(..) => {}
                CatalogObservation::Outcome(..) => panic!("an outcome before the catalog"),
            }
        }
    }

    /// Revokes the epoch through the transport's own path: every waiting
    /// read answers `ESTALE`, owed replies flush, the socket closes.
    fn revoke(&mut self) {
        self.transport.disconnect(&mut self.registry).unwrap();
        self.revoked = true;
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        if !self.revoked {
            let _ = self.transport.disconnect(&mut self.registry);
        }
        self.registry.collect();
        for directory in &self.directories {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

// ---- records ----

fn demand(demand_id: u64) -> ShellContentRecord {
    ShellContentRecord::FrameDemand(ContentFrameDemand {
        grant: grant(),
        output: output(),
        allocation: ContentAllocationId::default(),
        demand_id,
        reason: 1,
    })
}

fn enqueue_demand(live: &mut Live, demand_id: u64) -> Ticket {
    let admission = live
        .client
        .enqueue_content_tracked(tx(100 + demand_id), &demand(demand_id))
        .unwrap();
    assert_eq!(admission.count, 1);
    admission.first
}

fn action(event_id: u64) -> ContentAction {
    ContentAction {
        grant: grant(),
        output: output(),
        candidate_generation: 1,
        presentation_epoch: 1,
        interaction_generation: 1,
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        target_id: 1,
        target_generation: 1,
        action_id: 1,
        event_id,
        kind: 1,
        reason: 0,
    }
}

fn ack(action: &ContentAction) -> ContentActionAck {
    ContentActionAck {
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
    }
}

fn snapshot(generation: u64) -> ShellIndicatorSnapshot {
    ShellIndicatorSnapshot {
        connection_epoch: EPOCH,
        generation,
        active_output: Some(OutputId::from_raw(3)),
        statuses: vec![ShellOutputStatus {
            output: OutputId::from_raw(3),
            focus_bits: 1,
            layout: "single".to_owned(),
        }],
        indicators: vec![ShellIndicator {
            output: OutputId::from_raw(3),
            indicator: 1,
            action: 2,
            slot: 0,
            state_bits: 1,
            label: format!("view {generation}"),
        }],
    }
}

fn indicator_activation(generation: u64, event_id: u64) -> ShellIndicatorActivation {
    ShellIndicatorActivation {
        connection_epoch: EPOCH,
        snapshot_generation: generation,
        output: OutputId::from_raw(3),
        indicator: 1,
        action: 2,
        event_id,
    }
}

/// 4096 entries at their text maxima, each with an identity: the largest r8
/// catalog, far past one `msize`.
fn maximal_catalog(generation: u64) -> ShellPersistentCatalog {
    let label = format!("{generation}").repeat(128)[..128].to_owned();
    let keywords = "K".repeat(256);
    // Keep every identity at the maximum byte length while preserving the
    // persistent catalog's one-name-per-slot rule.
    let tail = "I".repeat(SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES - "registered:".len() - 4);
    let entries: Vec<ShellApplicationDescriptor> = (1..=SOPHIA_SHELL_MAX_APPLICATIONS as u16)
        .map(|slot| ShellApplicationDescriptor {
            slot,
            available: slot % 2 == 1,
            label: label.clone(),
            keywords: keywords.clone(),
        })
        .collect();
    let identities = entries
        .iter()
        .map(|entry| (entry.slot, format!("registered:{:04}{tail}", entry.slot)))
        .collect();
    ShellPersistentCatalog {
        catalog: ShellApplicationCatalog {
            connection_epoch: EPOCH,
            generation,
            entries,
        },
        identities,
    }
}

/// The kind and submission ID of a staged candidate record.
fn staged_kind(record: &[u8]) -> (ShellFileKind, u64) {
    let header = decode_shell_file_record(record, ShellFileClass::Candidate)
        .unwrap()
        .header;
    (header.kind, header.submission_id)
}

/// Rounds until both tickets of a response pair are `Submitted`, returning
/// the round each settled in: the ACK's must come strictly first.
fn settle_pair(live: &mut Live, admission: Admission) -> (Ticket, Ticket) {
    let tickets: Vec<Ticket> = admission.tickets().collect();
    assert_eq!(tickets.len(), 2, "an ACK and its activation: two tickets");
    let (ack, activation) = (tickets[0], tickets[1]);
    assert_eq!(activation.0, ack.0 + 1, "consecutive tickets");
    let mut settled = [None, None];
    let mut round = 0usize;
    live.until(|live| {
        round += 1;
        for (index, ticket) in [ack, activation].into_iter().enumerate() {
            if settled[index].is_none() && live.custody(ticket) == Some(Custody::Submitted) {
                settled[index] = Some(round);
            }
            assert!(
                matches!(
                    live.custody(ticket),
                    Some(Custody::Queued | Custody::InFlight | Custody::Submitted)
                ),
                "{ticket:?}: {:?}",
                live.custody(ticket)
            );
        }
        settled.iter().all(Option::is_some)
    });
    assert!(
        settled[0] < settled[1],
        "the ACK settled before its activation: {settled:?}"
    );
    assert_eq!(live.custody(ack), Some(Custody::Submitted));
    assert_eq!(live.custody(activation), Some(Custody::Submitted));
    (ack, activation)
}

// ---- 1. the catalog object ----

/// A maximal r8 catalog, far larger than `msize`, is read across many reads
/// and delivered once with an exact value; its announcement is acknowledged
/// only after the fetch. A newer generation then delivers exactly that one.
#[test]
fn a_maximal_catalog_is_read_across_reads_delivered_once_and_acknowledged_after() {
    let mut live = Live::connect(Role::Dock);
    let mut inbox = CatalogInbox::new(EPOCH).unwrap();
    let first = maximal_catalog(8);
    live.transport
        .publish_catalog(&live.registry, tx(1), &first)
        .unwrap();
    assert_eq!(live.next_catalog(&mut inbox), (tx(1), first));

    let announced = live
        .tap
        .seen
        .iter()
        .find_map(|seen| match seen {
            Seen::Event {
                sequence,
                published: Some(published),
                ..
            } if published.object == ShellFileKind::Catalog => Some(*sequence),
            _ => None,
        })
        .expect("the catalog announcement");
    let reads: Vec<(usize, u64, usize)> = live
        .tap
        .seen
        .iter()
        .enumerate()
        .filter_map(|(at, seen)| match seen {
            Seen::ObjectRead {
                node,
                offset,
                returned,
            } if node == "catalog" => Some((at, *offset, *returned)),
            _ => None,
        })
        .collect();
    assert!(reads.len() > 2, "several reads: {}", reads.len());
    let mut offset = 0;
    for (_, at, returned) in &reads {
        assert_eq!(*at, offset, "contiguous reads");
        offset += *returned as u64;
    }
    assert!(offset > 65536, "{offset} bytes: more than one msize");
    assert_eq!(reads.last().unwrap().2, 0, "a zero read ended the object");
    live.until(|live| live.tap.acks().iter().any(|&ack| ack >= announced));
    let first_covering = live
        .tap
        .seen
        .iter()
        .position(|seen| matches!(seen, Seen::Ack(sequence) if *sequence >= announced))
        .unwrap();
    assert!(
        first_covering > reads.last().unwrap().0,
        "the announcement was acknowledged before its object was fetched"
    );

    let newer = maximal_catalog(9);
    live.transport
        .publish_catalog(&live.registry, tx(2), &newer)
        .unwrap();
    assert_eq!(live.next_catalog(&mut inbox), (tx(2), newer));
    for _ in 0..8 {
        live.turn().unwrap();
        assert!(
            !matches!(
                live.client.take_catalog_observation(&mut inbox).unwrap(),
                Some(CatalogObservation::Catalog(..))
            ),
            "a catalog delivered twice"
        );
    }
}

// ---- 2. the indicators object ----

#[test]
fn published_indicators_are_delivered_once_and_exactly() {
    let mut live = Live::connect(Role::Bar(INDICATORS));
    live.transport
        .publish_indicators(&live.registry, tx(5), &snapshot(4))
        .unwrap();
    assert_eq!(
        live.next(ShellConnection::take_indicators),
        (tx(5), snapshot(4))
    );
    for _ in 0..8 {
        live.turn().unwrap();
        assert_eq!(live.client.take_indicators(), Ok(None), "delivered twice");
    }
}

// ---- 3. role family records and outcomes ----

/// A catalog candidate reaches `Submitted`; an ACK and the catalog
/// activation it authorizes go as two tickets, the ACK's settling first;
/// the transport's `CatalogActivationOutcome` arrives typed and exact.
#[test]
fn a_catalog_candidate_and_action_response_reach_custody_and_the_outcome_arrives_typed() {
    let mut live = Live::connect(Role::Dock);
    let mut lifecycle = ContentLifecycle::new(dock_limits()).unwrap();
    let begin = CatalogCandidateBegin {
        content: ContentCandidateBegin {
            grant: grant(),
            candidate_generation: 1,
            output: output(),
            facts_generation: 1,
            pacing_permit: 1,
            interaction_generation: 1,
            surface_count: 0,
            placement_count: 0,
            target_count: 0,
        },
        catalog_generation: 9,
    };
    let end = ContentCandidateEnd {
        grant: grant(),
        candidate_generation: 1,
        surface_count: 0,
        placement_count: 0,
        target_count: 0,
    };
    let candidate = live
        .client
        .enqueue_catalog_candidate_tracked(&mut lifecycle, tx(20), &begin, &[], &end)
        .unwrap();
    assert_eq!(candidate.count, 1, "one whole record");
    assert_eq!(live.settle(candidate.first), Custody::Submitted);

    let activation = CatalogActivation {
        action: action(30),
        catalog_generation: 9,
    };
    let response = live
        .client
        .enqueue_catalog_action_response_tracked(
            tx(21),
            &ack(&activation.action),
            Some((tx(22), &activation)),
        )
        .unwrap();
    settle_pair(&mut live, response);
    let kinds: Vec<ShellFileKind> = live
        .tap
        .staged()
        .iter()
        .map(|record| staged_kind(record).0)
        .collect();
    assert_eq!(
        kinds,
        [
            ShellFileKind::CatalogCandidate,
            ShellFileKind::ActionAck,
            ShellFileKind::CatalogActivate
        ]
    );

    // The transport's intake hands the activation to Session; the status
    // below stands in for Session's decision and is opaque here.
    let deadline = Instant::now() + WAIT;
    let (transaction, taken) = loop {
        live.tap.pump();
        if let Some(taken) = live
            .transport
            .connection(&mut live.registry)
            .poll_catalog_activation()
            .unwrap()
        {
            break taken;
        }
        live.tap.pump();
        live.client.poll_io().unwrap();
        assert!(
            Instant::now() < deadline,
            "the activation never reached intake"
        );
    };
    assert_eq!((transaction, &taken), (tx(22), &activation));
    const OPAQUE_STATUS: u16 = 2;
    live.transport
        .connection(&mut live.registry)
        .finish_catalog_activation(transaction, &taken, OPAQUE_STATUS)
        .unwrap();
    let mut inbox = CatalogInbox::new(EPOCH).unwrap();
    let outcome = loop {
        match live.next(|client| client.take_catalog_observation(&mut inbox)) {
            CatalogObservation::Outcome(transaction, outcome) => break (transaction, outcome),
            CatalogObservation::Content(..) => {}
            CatalogObservation::Catalog(..) => panic!("no catalog was published"),
        }
    };
    assert_eq!(
        outcome,
        (
            tx(22),
            CatalogActivationOutcome {
                activation,
                status: OPAQUE_STATUS,
                reason: 0,
            }
        )
    );
}

/// An indicator activation alone, then an ACK paired with one: every
/// ticket reaches `Submitted`, the pair's ACK first; the transport's
/// `IndicatorActivationOutcome` for each arrives typed and exact.
#[test]
fn indicator_activations_reach_custody_and_their_outcomes_arrive_typed() {
    let mut live = Live::connect(Role::Bar(INDICATORS | INDICATOR_ACTIVATION));
    live.transport
        .publish_indicators(&live.registry, tx(5), &snapshot(4))
        .unwrap();
    live.next(ShellConnection::take_indicators);

    let alone = live
        .client
        .enqueue_indicator_activation_tracked(tx(9), &indicator_activation(4, 11))
        .unwrap();
    assert_eq!(alone.count, 1);
    assert_eq!(live.settle(alone.first), Custody::Submitted);

    let paired = indicator_activation(4, 12);
    let mut consumed = action(paired.event_id);
    consumed.output.id = paired.output.raw();
    consumed.target_id = paired.indicator;
    consumed.action_id = paired.action;
    let response = live
        .client
        .enqueue_indicator_action_response_tracked(tx(10), &ack(&consumed), Some((tx(11), &paired)))
        .unwrap();
    settle_pair(&mut live, response);
    let kinds: Vec<ShellFileKind> = live
        .tap
        .staged()
        .iter()
        .map(|record| staged_kind(record).0)
        .collect();
    assert_eq!(
        kinds,
        [
            ShellFileKind::IndicatorActivate,
            ShellFileKind::ActionAck,
            ShellFileKind::IndicatorActivate
        ]
    );

    for (transaction, activation) in [(tx(9), indicator_activation(4, 11)), (tx(11), paired)] {
        let deadline = Instant::now() + WAIT;
        let taken = loop {
            live.tap.pump();
            if let Some(taken) = live
                .transport
                .poll_indicator_activation(&mut live.registry)
                .unwrap()
            {
                break taken;
            }
            live.tap.pump();
            live.client.poll_io().unwrap();
            assert!(
                Instant::now() < deadline,
                "the activation never reached intake"
            );
        };
        assert_eq!(taken, (transaction, activation));
        // Opaque stand-in for Session's decision; only carriage is asserted.
        live.transport
            .finish_indicator_activation(
                &mut live.registry,
                transaction,
                &activation,
                ShellIndicatorActivationStatus::Stale,
                0,
            )
            .unwrap();
        assert_eq!(
            live.next(ShellConnection::take_indicator_activation_outcome),
            (
                transaction,
                ShellIndicatorActivationOutcome {
                    connection_epoch: EPOCH,
                    snapshot_generation: 4,
                    event_id: activation.event_id,
                    status: ShellIndicatorActivationStatus::Stale,
                    reason: 0,
                }
            )
        );
    }
}

// ---- 4. a live EAGAIN ----

/// The real journal refuses a submit with `EAGAIN` once its unsolicited
/// room is spent while the client, its inbox full, has stopped reading and
/// acknowledging. The tap shows the submit issued and refused; the unit
/// stays `InFlight`; once the caller drains its inbox the client reads,
/// acknowledges and sends the same submit again, which reaches `Submitted`
/// under the same submission ID, journaled once.
#[test]
fn a_submit_the_full_journal_refuses_with_eagain_reaches_submitted_unchanged() {
    let mut live = Live::connect(Role::Bar(INDICATORS));
    // Output facts fill the client's inbox (the limits record plus 63
    // snapshots), each fetched before the next is published.
    for generation in 1..=63u64 {
        live.transport
            .publish_content_output_facts(
                &mut live.registry,
                tx(generation),
                generation,
                vec![ContentOutputFactsEntry {
                    output: output(),
                    local_width: 64 + generation as u32,
                    local_height: 64,
                    scale_numerator: 1,
                    scale_denominator: 1,
                    scale_generation: 1,
                }],
            )
            .unwrap();
        live.until(|live| {
            live.tap
                .seen
                .iter()
                .filter(|seen| {
                    matches!(seen, Seen::ObjectRead { node, returned: 0, .. } if node == "outputs")
                })
                .count()
                == generation as usize
        });
    }
    // With the client no longer reading, announcements spend the journal's
    // unsolicited room until the transport cannot publish another.
    let mut generation = 1;
    loop {
        match live.transport.publish_indicators(
            &live.registry,
            tx(1000 + generation),
            &snapshot(generation),
        ) {
            Ok(()) => generation += 1,
            Err(ShellTransportError::ActivationQueueSaturated) => break,
            Err(error) => panic!("publish: {error}"),
        }
        live.turn().unwrap();
        assert!(generation < 400, "the journal never filled");
    }
    let staged_before = live.tap.staged().len();

    let ticket = enqueue_demand(&mut live, 1);
    live.until(|live| live.tap.submit_replies().contains(&Err(EAGAIN)));
    live.until(|live| live.client.wake_deadline().is_some());
    assert_eq!(live.custody(ticket), Some(Custody::InFlight));
    let refused = live.tap.submits();
    assert!(!refused.is_empty(), "the submit was issued");
    let bytes = refused[0].1.clone();
    let submission = decode_shell_file_submit(&bytes).unwrap().submission_id;
    assert_eq!(live.tap.submitted(submission), 0, "nothing journaled yet");

    // Progress: the caller drains its inbox, so the client reads and
    // acknowledges again, and the journal frees room.
    let deadline = Instant::now() + WAIT;
    while live.custody(ticket) != Some(Custody::Submitted) {
        while live.client.take_content().unwrap().is_some() {}
        live.turn().unwrap();
        assert!(Instant::now() < deadline, "custody never settled");
        assert!(matches!(
            live.custody(ticket),
            Some(Custody::InFlight | Custody::Submitted)
        ));
    }
    let submits = live.tap.submits();
    assert!(submits.len() >= 2, "sent again after the refusal");
    assert!(
        submits.iter().all(|(_, again)| *again == bytes),
        "every submit is the same 24 bytes"
    );
    assert_eq!(
        live.tap.submit_replies().last(),
        Some(&Ok(SHELL_FILE_SUBMIT_BYTES as u32))
    );
    assert_eq!(live.tap.submitted(submission), 1, "journaled exactly once");
    let staged = &live.tap.staged()[staged_before..];
    assert_eq!(
        staged.len(),
        1,
        "the record was staged once, never rewritten"
    );
    assert_eq!(
        staged_kind(&staged[0]),
        (ShellFileKind::FrameDemand, submission)
    );
}

// ---- 5. a definitive refusal ----

/// The export refuses an `IndicatorActivate` from a bar that did not
/// negotiate indicator activation (bit 10) with `EACCES`; the client does
/// not refuse it locally. Custody is `Refused(13)`, and the next record
/// reaches `Submitted`.
#[test]
fn a_record_the_export_refuses_is_refused_and_the_lane_moves_on() {
    let mut live = Live::connect(Role::Bar(INDICATORS));
    let refused = live
        .client
        .enqueue_indicator_activation_tracked(tx(9), &indicator_activation(1, 1))
        .unwrap()
        .first;
    let following = enqueue_demand(&mut live, 1);
    assert_eq!(live.settle(refused), Custody::Refused(EACCES));
    assert_eq!(
        live.tap.submit_replies()[0],
        Err(EACCES),
        "the export refused it"
    );
    assert_eq!(live.settle(following), Custody::Submitted);
    let submission = decode_shell_file_submit(&live.tap.submits()[1].1)
        .unwrap()
        .submission_id;
    assert_eq!(live.tap.submitted(submission), 1);
}

// ---- 6. revocation ----

/// The transport revokes the epoch after the client issued its `submit`
/// but before the transport served it: the revocation's own turns answer
/// that submit `ESTALE`, so the unit's outcome is `Unknown`, and the unit
/// queued behind it never left the client.
#[test]
fn a_revocation_answering_the_issued_submit_leaves_it_unknown() {
    let mut live = Live::connect(Role::Bar(0));
    let issued = enqueue_demand(&mut live, 1);
    let queued = enqueue_demand(&mut live, 2);
    let deadline = Instant::now() + WAIT;
    // Rounds without letting the transport serve a submit once issued.
    while live.tap.submits().is_empty() {
        live.tap.pump();
        live.serve();
        live.tap.pump();
        live.client.poll_io().unwrap();
        live.tap.pump();
        assert!(Instant::now() < deadline, "the submit was never issued");
    }
    assert!(live.tap.submit_replies().is_empty(), "not yet served");
    live.revoke();
    assert_eq!(live.settle(issued), Custody::Unknown);
    assert_eq!(
        live.tap.submit_replies(),
        [Err(ESTALE)],
        "the transport saw it"
    );
    assert_eq!(live.custody(queued), Some(Custody::DroppedUnsent));
}

/// The transport accepted the submit and journaled its `Submitted`, but the
/// epoch is revoked before either reply reaches the client: the unit's
/// outcome is `Unknown`, never `Submitted` and never `DroppedUnsent`.
#[test]
fn a_revocation_after_the_transport_took_custody_leaves_it_unknown() {
    let mut live = Live::connect(Role::Bar(0));
    live.tap.hold_after_submit = true;
    let issued = enqueue_demand(&mut live, 1);
    let queued = enqueue_demand(&mut live, 2);
    live.until(|live| !live.tap.submit_replies().is_empty());
    let submission = decode_shell_file_submit(&live.tap.submits()[0].1)
        .unwrap()
        .submission_id;
    assert_eq!(
        live.tap.submit_replies(),
        [Ok(SHELL_FILE_SUBMIT_BYTES as u32)],
        "the transport took custody"
    );
    live.until(|live| live.tap.submitted(submission) == 1);
    assert_eq!(live.custody(issued), Some(Custody::InFlight));
    live.revoke();
    assert_eq!(live.settle(issued), Custody::Unknown);
    assert_eq!(live.custody(queued), Some(Custody::DroppedUnsent));
}

// ---- 7. the ack that releases `transaction` ----

const EBUSY: u32 = 16;
const TWRITE: u8 = 118;

/// The next indicator activation the transport's intake hands to Session.
fn indicator_intake(live: &mut Live) -> (TransactionId, ShellIndicatorActivation) {
    let deadline = Instant::now() + WAIT;
    loop {
        live.tap.pump();
        if let Some(taken) = live
            .transport
            .poll_indicator_activation(&mut live.registry)
            .unwrap()
        {
            return taken;
        }
        live.tap.pump();
        live.client.poll_io().unwrap();
        assert!(
            Instant::now() < deadline,
            "the activation never reached intake"
        );
    }
}

/// The export keeps `transaction` busy (`EBUSY` on open) until the previous
/// submission's `Submitted` is acknowledged, and serves a connection's
/// requests in order. Delivery is delayed, never reordered, so that unit
/// A's `Submitted` is handled while an earlier ack (for event X) is still
/// unanswered, and X's ack reply then reaches the client in one drain with
/// whatever followed it (unit B's walk reply, were B's walk already sent).
/// The ack covering A must reach the transport before B's walk and open:
/// no `EBUSY`, and B reaches `Submitted`.
#[test]
fn the_next_record_opens_transaction_only_after_the_ack_releasing_it() {
    let mut live = Live::connect(Role::Bar(INDICATORS | INDICATOR_ACTIVATION));
    live.transport
        .publish_indicators(&live.registry, tx(5), &snapshot(4))
        .unwrap();
    live.next(ShellConnection::take_indicators);
    // An activation the transport takes now and answers later: the answer
    // is event X. Its status is an opaque stand-in for Session's decision.
    let prior = live
        .client
        .enqueue_indicator_activation_tracked(tx(9), &indicator_activation(4, 11))
        .unwrap()
        .first;
    assert_eq!(live.settle(prior), Custody::Submitted);
    let (transaction, activation) = indicator_intake(&mut live);
    for _ in 0..4 {
        live.turn().unwrap();
    }

    // A's submit, and every client byte after it, wait in the tap.
    live.tap.hold_up_from_submit = true;
    let a = enqueue_demand(&mut live, 1);
    let b = enqueue_demand(&mut live, 2);
    live.until(|live| live.tap.holding_up());

    // X reaches the client meanwhile; its ack queues behind A's submit.
    let acks = live.tap.acks().len();
    live.transport
        .finish_indicator_activation(
            &mut live.registry,
            transaction,
            &activation,
            ShellIndicatorActivationStatus::Stale,
            0,
        )
        .unwrap();
    live.until(|live| live.tap.acks().len() > acks);

    // The transport now serves A's submit, the events read and X's ack in
    // that order; its bytes from X's ack reply onward wait in the tap.
    live.tap.hold_down_from_ack_reply = true;
    live.tap.release_up();
    live.until(|live| live.custody(a) == Some(Custody::Submitted));
    let walks = live.tap.walks("transaction");
    for _ in 0..6 {
        live.turn().unwrap();
    }
    let held = live.tap.held_replies.clone();
    assert!(
        held.iter()
            .any(|(kind, node)| *kind == TWRITE && node == "ack"),
        "the scenario formed: X's ack reply is delayed ({held:?})"
    );
    let walked_early = live.tap.walks("transaction") > walks;

    // One drain: X's ack reply and everything delayed behind it.
    live.tap.release_down();
    let outcome = live.settle(b);
    let opens = live.tap.open_replies("transaction");
    assert!(
        !opens.contains(&Err(EBUSY)),
        "B's open of transaction was refused EBUSY (walked before the covering ack: \
         {walked_early}; delayed replies {held:?}; opens {opens:?}; custody {outcome:?})"
    );
    assert!(
        !walked_early,
        "B walked transaction before the ack covering A"
    );
    assert_eq!(outcome, Custody::Submitted);

    let submission = decode_shell_file_submit(&live.tap.submits()[0].1)
        .unwrap()
        .submission_id;
    let covering = live
        .tap
        .seen
        .iter()
        .find_map(|seen| match seen {
            Seen::Event {
                sequence,
                submitted: Some(value),
                ..
            } if value.submission_id == submission => Some(*sequence),
            _ => None,
        })
        .expect("A's Submitted");
    let acked = live
        .tap
        .seen
        .iter()
        .position(|seen| matches!(seen, Seen::Ack(sequence) if *sequence >= covering))
        .expect("an ack covering A's Submitted");
    let walked = live
        .tap
        .seen
        .iter()
        .rposition(|seen| *seen == Seen::Walk("transaction".to_owned()))
        .unwrap();
    assert!(acked < walked, "the covering ack preceded B's walk");
}
