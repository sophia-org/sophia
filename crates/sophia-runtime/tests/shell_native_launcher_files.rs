//! Native launcher (r7) end to end over the file wire: opening, allocation,
//! resource upload, a whole `NativeCandidate`, focus, semantic input and its
//! ack, a keyboard (Accept) activation and its outcome, then close. Reuses
//! the exact store-level fixtures the socket-wire native launcher suite
//! (`shell_native_launcher_transport.rs`, `support/native_launcher_content.rs`)
//! already proves; only the wire encoding differs here.
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;

#[allow(dead_code)] // Shared store fixture; this file drives only part of it.
#[path = "support/native_launcher_content.rs"]
mod fixtures;
use fixtures::*;

const MIB: u64 = 1024 * 1024;
const RLERROR: u8 = 7;
const EAGAIN: u32 = 11;
const ENOENT: u32 = 2;

const CAPS: u64 = SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
    | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER;

fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    }
}

fn transport() -> (ShellComponentTransport, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "shell-native-files-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    let transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    (transport, directory)
}

// A monotonic counter, not `Instant::now().elapsed()` (too coarse to stay
// unique across threads run concurrently by the test harness), so this
// regression test's socket directory never collides with a sibling test's.
static T252_NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn transport_t252() -> (ShellComponentTransport, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "shell-native-files-t252-{}-{}",
        std::process::id(),
        T252_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    (transport, directory)
}

fn header(kind: ShellFileKind, epoch: u64, id: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: epoch,
        submission_id: id,
        sequence: 0,
    }
}

fn errno(reply: (u8, Vec<u8>)) -> u32 {
    assert_eq!(reply.0, RLERROR);
    u32::from_le_bytes(reply.1[..4].try_into().unwrap())
}

fn negotiate(
    transport: &mut ShellComponentTransport,
    registry: &mut ContentEpochRegistry,
    epoch: u64,
    peer_done: &std::thread::JoinHandle<()>,
) -> ShellV1ServerWelcome {
    transport
        .begin_file_negotiation(registry, epoch, Duration::from_secs(2), granted())
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(welcome) = transport.poll_negotiation(registry, 64 * 1024).unwrap() {
            return welcome;
        }
        assert!(!peer_done.is_finished(), "peer ended before negotiation");
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
}

/// The whole native candidate built from the same `begin`/`chunk` fixtures
/// the socket suite submits as three separate frames.
fn native_candidate(permit: u64) -> NativeContentCandidate {
    let b = begin();
    let c = chunk();
    NativeContentCandidate {
        candidate: ContentCandidate {
            grant: b.content.grant,
            candidate_generation: b.content.candidate_generation,
            output: b.content.output,
            facts_generation: b.content.facts_generation,
            pacing_permit: permit,
            interaction_generation: b.content.interaction_generation,
            surfaces: c.surfaces,
            placements: c.placements,
            targets: c.targets,
        },
        opening: b.opening,
        catalog_generation: b.catalog_generation,
        state_revision: b.state_revision,
        selected: b.selected,
        rows: b.rows,
    }
}

#[test]
fn opening_allocation_candidate_focus_input_activation_and_close_cross_the_file_wire() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    transport
        .reserve_content_with_profile(&mut registry, limits(), ContentStoreProfile::NativeLauncher)
        .unwrap();
    let socket = transport.socket_path().to_owned();

    let (resource_and_candidate_tx, resource_and_candidate_rx) = mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();

        // Negotiate exactly the fixed native launcher capability set.
        let offer = encode_shell_file_negotiate(
            header(ShellFileKind::Negotiate, GRANT.connection_epoch, 1),
            ShellV1ClientHello {
                minimum_revision: 7,
                maximum_revision: 7,
                required_capabilities: CAPS,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        let value = decode_shell_file_negotiated(&negotiated).unwrap();
        assert_eq!(value.welcome.selected_revision, 7);
        assert_eq!(value.welcome.capabilities, CAPS);
        peer.ack(&negotiated);
        peer.open(6, b"limits", 0);
        assert_eq!(
            decode_shell_file_limits(&peer.read(6, 0)).unwrap(),
            limits()
        );

        // The root vocabulary: `catalog` exists for this profile (walks, but
        // is not yet published); `indicators` does not exist at all for a
        // launcher, so the walk itself is refused, never reaching an open.
        assert_eq!(errno(peer.open_path(20, &[b"catalog"], 0)), EAGAIN);
        let indicators_walk = [
            1u32.to_le_bytes().as_slice(),
            &21u32.to_le_bytes(),
            &1u16.to_le_bytes(),
            &10u16.to_le_bytes(),
            b"indicators".as_slice(),
        ]
        .concat();
        assert_eq!(errno(peer.rpc(110, &indicators_walk).unwrap()), ENOENT);

        // Opening: an event, not an object.
        let opening_event = peer.next_event();
        let value = decode_shell_file_native_launcher_transaction(
            &opening_event,
            ShellFileKind::NativeOpening,
        )
        .unwrap();
        assert_eq!(value.record, ShellNativeLauncherRecord::Opening(opening()));
        peer.ack(&opening_event);

        // Outputs: a pinned object, announced by its own event.
        let published = peer.next_event();
        let announced = decode_shell_file_object_published(&published).unwrap();
        assert_eq!(announced.object, ShellFileKind::Outputs);
        peer.ack(&published);
        peer.open(7, b"outputs", 0);
        let ShellContentRecord::OutputFacts(outputs) =
            decode_shell_file_outputs(&peer.read(7, 0)).unwrap().record
        else {
            panic!("output facts");
        };
        assert_eq!(outputs.outputs[0].output, OUTPUT);

        // Allocation.
        let allocation_request = encode_shell_file_native_launcher_transaction(
            header(
                ShellFileKind::NativeAllocationRequest,
                GRANT.connection_epoch,
                2,
            ),
            &ShellFileNativeLauncherRecord {
                transaction: tx(20),
                record: ShellNativeLauncherRecord::AllocationRequest(request(1)),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&allocation_request, 2);
        let event = peer.next_event();
        let value = decode_shell_file_allocation_result(&event).unwrap();
        let ShellContentRecord::AllocationResult(result) = value.record else {
            panic!("allocation result");
        };
        assert_eq!(result.status, 1);
        peer.ack(&event);

        // Resource upload for the candidate's one placement.
        let begin = encode_shell_file_resource_begin(
            header(ShellFileKind::ResourceBegin, GRANT.connection_epoch, 3),
            &ShellFileResourceBegin {
                transaction: tx(21),
                slot: 0,
                record: ShellContentRecord::ResourceBegin(ContentResourceBegin {
                    grant: GRANT,
                    resource: RESOURCE,
                    width_px: 2,
                    height_px: 1,
                    rendered_scale_numerator: 1,
                    rendered_scale_denominator: 1,
                    pixel_format: 1,
                    chunk_count: 1,
                    total_bytes: 8,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&begin, 3);
        let admitted = peer.next_event();
        let ShellContentRecord::ResourceStatus(status) =
            decode_shell_file_resource_status(&admitted).unwrap().record
        else {
            panic!("resource status");
        };
        assert_eq!(status.status, 1);
        peer.ack(&admitted);
        assert_eq!(peer.open_path(10, &[b"upload", b"0"], 1).0, 13);
        assert_eq!(
            peer.write_at(10, 0, &[0, 0, 255, 255, 0, 128, 0, 128]).0,
            119
        );
        let end = encode_shell_file_resource_end(
            header(ShellFileKind::ResourceEnd, GRANT.connection_epoch, 4),
            &ShellFileTransactionRecord {
                transaction: tx(22),
                record: ShellContentRecord::ResourceEnd(ContentResourceEnd {
                    grant: GRANT,
                    resource: RESOURCE,
                    total_bytes: 8,
                    chunk_count: 1,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&end, 4);
        let accepted = peer.next_event();
        let ShellContentRecord::ResourceStatus(status) =
            decode_shell_file_resource_status(&accepted).unwrap().record
        else {
            panic!("resource status");
        };
        assert_eq!(status.status, 2);
        peer.ack(&accepted);

        // Pacing: demand then permit.
        let demand = encode_shell_file_transaction(
            header(ShellFileKind::FrameDemand, GRANT.connection_epoch, 5),
            &ShellFileTransactionRecord {
                transaction: tx(23),
                record: ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant: GRANT,
                    output: OUTPUT,
                    allocation: ALLOCATION,
                    demand_id: 1,
                    reason: 1,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&demand, 5);
        let event = peer.next_event();
        let value = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
        let ShellContentRecord::FramePermit(permit) = value.record else {
            panic!("frame permit");
        };
        assert_eq!(permit.state, 1);
        peer.ack(&event);

        // The whole native candidate, naming the granted permit.
        let candidate_bytes = encode_shell_file_native_candidate(
            header(ShellFileKind::NativeCandidate, GRANT.connection_epoch, 6),
            &ShellFileNativeCandidate {
                transaction: tx(24),
                candidate: native_candidate(permit.permit_id),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&candidate_bytes, 6);
        resource_and_candidate_tx.send(()).unwrap();

        let mut outcomes = Vec::new();
        while outcomes.len() < 2 {
            let event = peer.next_event();
            let value =
                decode_shell_file_transaction(&event, ShellFileKind::CandidateOutcome).unwrap();
            let ShellContentRecord::CandidateOutcome(outcome) = value.record else {
                panic!("candidate outcome");
            };
            outcomes.push(outcome.kind);
            peer.ack(&event);
        }
        assert_eq!(outcomes, vec![1, 2]);

        // Focus: Session-initiated, an event naming the fresh binding.
        let event = peer.next_event();
        let value =
            decode_shell_file_native_launcher_transaction(&event, ShellFileKind::NativeFocus)
                .unwrap();
        let ShellNativeLauncherRecord::Focus(focus) = value.record else {
            panic!("focus");
        };
        assert_eq!(focus.grant, GRANT);
        assert_eq!(focus.opening, opening().opening);
        peer.ack(&event);

        // Semantic input: Session-initiated, then this peer's ack. Accept
        // (Enter on the selected row) carries no text.
        let event = peer.next_event();
        let value = decode_shell_file_native_input(&event).unwrap();
        let ShellNativeLauncherRecord::Input(input) = value.record else {
            panic!("input");
        };
        assert_eq!(input.kind, NativeLauncherInputKind::Accept);
        assert_eq!(input.text, "");
        peer.ack(&event);
        let ack = encode_shell_file_native_launcher_transaction(
            header(ShellFileKind::NativeInputAck, GRANT.connection_epoch, 7),
            &ShellFileNativeLauncherRecord {
                transaction: tx(25),
                record: ShellNativeLauncherRecord::InputAck(NativeLauncherInputAck {
                    event: input.event,
                    disposition: 1,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&ack, 7);

        // Keyboard activation of the presented selection (Accept => cause 1),
        // echoing the exact acknowledged event, then its outcome.
        let activate = encode_shell_file_native_launcher_transaction(
            header(ShellFileKind::NativeActivate, GRANT.connection_epoch, 8),
            &ShellFileNativeLauncherRecord {
                transaction: tx(26),
                record: ShellNativeLauncherRecord::Activate(NativeLauncherActivation {
                    event: input.event,
                    cause: 1,
                    slot: 2,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&activate, 8);
        let event = peer.next_event();
        let value = decode_shell_file_native_launcher_transaction(
            &event,
            ShellFileKind::NativeActivationOutcome,
        )
        .unwrap();
        let ShellNativeLauncherRecord::ActivationOutcome(outcome) = value.record else {
            panic!("activation outcome");
        };
        assert_eq!(outcome.status, 1);
        peer.ack(&event);

        // Close: focus revocation, then Closed.
        let event = peer.next_event();
        let value = decode_shell_file_native_launcher_transaction(
            &event,
            ShellFileKind::NativeFocusRevoked,
        )
        .unwrap();
        assert!(matches!(
            value.record,
            ShellNativeLauncherRecord::FocusRevoked(_)
        ));
        peer.ack(&event);
        let event = peer.next_event();
        let value =
            decode_shell_file_native_launcher_transaction(&event, ShellFileKind::NativeClosed)
                .unwrap();
        assert!(matches!(value.record, ShellNativeLauncherRecord::Closed(_)));
        peer.ack(&event);
    });

    let start = Instant::now();
    let welcome = negotiate(&mut transport, &mut registry, GRANT.connection_epoch, &peer);
    assert_eq!(welcome.capabilities, CAPS);
    assert!(transport.supports_native_launcher());

    transport
        .publish_native_launcher_opening(&registry, tx(2), opening())
        .unwrap();
    transport
        .publish_content_output_facts(&mut registry, tx(1), 5, vec![facts()])
        .unwrap();

    let empty_allocations: Vec<ContentAllocationSnapshot> = Vec::new();
    let ctx = context(&empty_allocations);
    let catalog_value = catalog();
    let native_ctx = native(&catalog_value);
    while transport
        .next_content_allocation_request(&registry)
        .is_none()
    {
        transport
            .service_native_launcher_content(&mut registry, ctx, native_ctx, 0)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    transport
        .grant_content_allocation(&mut registry, 1, allocation(), &[])
        .unwrap();

    // Resource upload, pacing and the whole candidate: drive the shared
    // servicer, granting the pacing demand as soon as it appears, until the
    // peer has finished submitting everything and the servicer has nothing
    // left ready.
    let allocations = [allocation()];
    let ctx = context(&allocations);
    let mut granted_demand = false;
    loop {
        let processed = transport
            .service_native_launcher_content(&mut registry, ctx, native_ctx, 0)
            .unwrap();
        if !granted_demand
            && let Some((demand_tx, demand)) = transport.next_content_demand(&registry)
        {
            transport
                .grant_content_demand(&mut registry, demand_tx, demand.output, 1, 0)
                .unwrap();
            granted_demand = true;
        }
        if processed == 0 && granted_demand && resource_and_candidate_rx.try_recv().is_ok() {
            while transport
                .service_native_launcher_content(&mut registry, ctx, native_ctx, 0)
                .unwrap()
                > 0
            {}
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }

    let render = transport
        .begin_native_launcher_submission(&mut registry, 1, ctx, native_ctx, 0)
        .unwrap();
    assert_eq!(render.resource(RESOURCE).unwrap().bytes().len(), 8);
    transport
        .content_prepared(&mut registry, GRANT, OUTPUT, 1, 1, 1, 0)
        .unwrap();
    transport
        .content_presented(&mut registry, GRANT, OUTPUT, 1, 9, 1, 1)
        .unwrap();
    drop(render);

    let focus = transport
        .install_native_launcher_focus(&mut registry, tx(30))
        .unwrap();

    // Accept (Enter on the selected row) does not advance the state
    // revision, unlike every other input kind, so the keyboard activation
    // that follows still names the exact revision focus was installed
    // against and is not rejected as stale.
    transport
        .issue_native_launcher_input(
            &mut registry,
            focus,
            tx(31),
            NativeLauncherInputKind::Accept,
            "",
            10,
        )
        .unwrap();
    let ack_transaction;
    let ack;
    loop {
        if let Some((transaction, value, found)) = transport
            .poll_native_launcher_input_ack(&mut registry)
            .unwrap()
        {
            assert!(found);
            ack_transaction = transaction;
            ack = value;
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    assert_eq!(ack_transaction, tx(25));
    assert_eq!(ack.disposition, 1);

    let (activation_transaction, activation);
    loop {
        if let Some((transaction, value)) = transport
            .poll_native_launcher_activation(&mut registry)
            .unwrap()
        {
            activation_transaction = transaction;
            activation = value;
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    let eligibility = transport
        .native_launcher_activation_eligibility(activation_transaction, &activation, &catalog(), 20)
        .unwrap();
    assert!(matches!(
        eligibility,
        NativeLauncherActivationEligibility::Keyboard
    ));
    transport
        .finish_native_launcher_activation(
            &registry,
            activation_transaction,
            &activation,
            NativeLauncherActivationDecision::Admitted,
        )
        .unwrap();

    transport
        .close_native_launcher(&mut registry, opening(), tx(32), ContentReason::Revoked)
        .unwrap();

    while !peer.is_finished() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}

// Regression for t252 fix A (native): the owner's rejecting outcome for the
// earlier part of a whole `NativeCandidate` must be the candidate's only
// outcome, and the connection must stay usable. Before the fix, the
// exploded Chunk/End of a Begin-rejected candidate still reached the store
// on later service visits, found no matching assembly and returned an
// unreported `Stale`, which `service_native_launcher_content` propagated as
// an `Err` -- the signal callers use to revoke the whole component.
#[test]
fn a_stale_catalog_generation_native_candidate_is_rejected_once_and_the_connection_survives() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport_t252();
    transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    transport
        .reserve_content_with_profile(&mut registry, limits(), ContentStoreProfile::NativeLauncher)
        .unwrap();
    let socket = transport.socket_path().to_owned();

    let (submitted_tx, submitted_rx) = mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();

        let offer = encode_shell_file_negotiate(
            header(ShellFileKind::Negotiate, GRANT.connection_epoch, 1),
            ShellV1ClientHello {
                minimum_revision: 7,
                maximum_revision: 7,
                required_capabilities: CAPS,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        decode_shell_file_negotiated(&negotiated).unwrap();
        peer.ack(&negotiated);

        let opening_event = peer.next_event();
        let value = decode_shell_file_native_launcher_transaction(
            &opening_event,
            ShellFileKind::NativeOpening,
        )
        .unwrap();
        assert_eq!(value.record, ShellNativeLauncherRecord::Opening(opening()));
        peer.ack(&opening_event);

        let permit_a = {
            let event = peer.next_event();
            let value = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
            let ShellContentRecord::FramePermit(p) = value.record else {
                panic!("frame permit");
            };
            peer.ack(&event);
            p
        };

        // Otherwise exactly the working `native_candidate` shape (a valid
        // permit, one row-consistent surface/target), naming a
        // catalog_generation the store never published.
        let mut stale = native_candidate(permit_a.permit_id);
        stale.catalog_generation = 9999;
        let stale_bytes = encode_shell_file_native_candidate(
            header(ShellFileKind::NativeCandidate, GRANT.connection_epoch, 2),
            &ShellFileNativeCandidate {
                transaction: tx(24),
                candidate: stale,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&stale_bytes, 2);
        submitted_tx.send(()).unwrap();

        // Exactly one outcome for the stale candidate: Rejected/Stale. Its
        // Chunk and End must never surface as if a fresh candidate began.
        let event = peer.next_event();
        let value = decode_shell_file_transaction(&event, ShellFileKind::CandidateOutcome).unwrap();
        let ShellContentRecord::CandidateOutcome(outcome) = value.record else {
            panic!("candidate outcome");
        };
        assert_eq!(outcome.kind, 3);
        assert_eq!(outcome.reason, ContentReason::Stale as u16);
        peer.ack(&event);

        let permit_b = {
            let event = peer.next_event();
            let value = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
            let ShellContentRecord::FramePermit(p) = value.record else {
                panic!("frame permit");
            };
            peer.ack(&event);
            p
        };

        // A fresh, fully valid candidate on the very same connection: the
        // discarded parts above did not leak, and the store was not revoked.
        let mut valid = native_candidate(permit_b.permit_id);
        valid.candidate.candidate_generation = 2;
        let valid_bytes = encode_shell_file_native_candidate(
            header(ShellFileKind::NativeCandidate, GRANT.connection_epoch, 3),
            &ShellFileNativeCandidate {
                transaction: tx(25),
                candidate: valid,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&valid_bytes, 3);

        let event = peer.next_event();
        let value = decode_shell_file_transaction(&event, ShellFileKind::CandidateOutcome).unwrap();
        let ShellContentRecord::CandidateOutcome(outcome) = value.record else {
            panic!("candidate outcome");
        };
        assert_eq!(outcome.kind, 1);
        peer.ack(&event);
    });

    let start = Instant::now();
    let welcome = negotiate(&mut transport, &mut registry, GRANT.connection_epoch, &peer);
    assert_eq!(welcome.capabilities, CAPS);
    assert!(transport.supports_native_launcher());

    transport
        .publish_native_launcher_opening(&registry, tx(2), opening())
        .unwrap();
    // The candidate's allocation and resource are supplied directly, as the
    // store-level fixtures already do for the socket wire's own suite; no
    // allocation or resource handshake crosses the wire in this test.
    resources(&mut registry);

    let allocations = [allocation()];
    let ctx = context(&allocations);
    let catalog_value = catalog();
    let native_ctx = native(&catalog_value);

    transport
        .grant_content_permit(&mut registry, tx(10), OUTPUT, 1, 1, 0)
        .unwrap();
    // Drive the stale candidate. The fix means only its Begin is ever
    // serviced -- the discarded Chunk/End leave nothing further queued --
    // and the call keeps returning `Ok` even though the Begin was refused.
    loop {
        let processed = transport
            .service_native_launcher_content(&mut registry, ctx, native_ctx, 1)
            .unwrap();
        if processed == 0 && submitted_rx.try_recv().is_ok() {
            while transport
                .service_native_launcher_content(&mut registry, ctx, native_ctx, 1)
                .unwrap()
                > 0
            {}
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }

    transport
        .grant_content_permit(&mut registry, tx(11), OUTPUT, 2, 2, 2)
        .unwrap();
    let mut processed = 0;
    while processed < 3 {
        processed += transport
            .service_native_launcher_content(&mut registry, ctx, native_ctx, 3)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    let render = transport
        .begin_native_launcher_submission(&mut registry, 2, ctx, native_ctx, 4)
        .unwrap();
    transport
        .content_prepared(&mut registry, GRANT, OUTPUT, 2, 1, 1, 5)
        .unwrap();
    drop(render);

    while !peer.is_finished() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}
