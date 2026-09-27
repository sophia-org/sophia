//! Regression for t252 fix A (base content) on the file wire: split into its
//! own file (rather than grown into shell_file_transport.rs) to stay under
//! the source-layout debt ledger's line ceiling. Reuses the exact
//! transport/negotiate fixtures shell_file_transport.rs defines for its own
//! base-content suite; only the wire encoding matters here, not the profile.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;

const MIB: u64 = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

fn transport() -> (ShellComponentTransport, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "shell-files-discard-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
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
    (transport, directory)
}

fn limits(epoch: u64) -> ContentLimits {
    ContentLimits::prototype(ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    })
}

fn hello() -> ShellV1ClientHello {
    ShellV1ClientHello {
        minimum_revision: 5,
        maximum_revision: 6,
        required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
    }
}

fn candidate(kind: ShellFileKind, epoch: u64, id: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: epoch,
        submission_id: id,
        sequence: 0,
    }
}

fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: false,
    }
}

fn negotiate(
    transport: &mut ShellComponentTransport,
    registry: &mut ContentEpochRegistry,
    epoch: u64,
    policy: ShellContentAdmissionPolicy,
    peer_done: &std::thread::JoinHandle<()>,
) -> Result<Option<ShellV1ServerWelcome>, ShellTransportError> {
    transport.begin_file_negotiation(registry, epoch, Duration::from_secs(2), policy)?;
    let start = Instant::now();
    loop {
        if let Some(welcome) = transport.poll_negotiation(registry, 64 * 1024)? {
            return Ok(Some(welcome));
        }
        if peer_done.is_finished() {
            return Ok(None);
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
}

/// A trivial, structurally valid whole `Candidate`: no surfaces, placements
/// or targets, so it needs no allocation or resource to reach `pending`.
fn trivial_candidate(
    generation: u64,
    grant: ContentGrant,
    output: ContentOutputId,
    permit: u64,
) -> ContentCandidate {
    ContentCandidate {
        grant,
        candidate_generation: generation,
        output,
        facts_generation: 1,
        pacing_permit: permit,
        interaction_generation: 1,
        surfaces: vec![],
        placements: vec![],
        targets: vec![],
    }
}

// Regression for t252 fix A (base content): a Begin rejected with an
// outcome -- here, a stale candidate_generation, never the permit-existence
// check `a_candidate_without_its_permit_is_rejected_through_its_outcome` in
// shell_file_transport.rs already covers -- must be the candidate's only
// outcome, discarding its exploded Chunk/End rather than letting them reach
// the store as if a fresh candidate had begun, and the service call must
// keep returning `Ok`.
#[test]
fn a_stale_generation_candidate_is_rejected_once_and_a_later_one_still_prepares() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(1);
    transport
        .reserve_content(&mut registry, expected.clone())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let grant = expected.grant;
    let output = ContentOutputId {
        id: 2,
        generation: 1,
    };

    let (submitted_tx, submitted_rx) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 1), hello())
            .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        decode_shell_file_negotiated(&negotiated).unwrap();
        peer.ack(&negotiated);

        // A: trivial and fully valid, establishing a baseline
        // candidate_generation (5) so a later Begin naming an
        // equal-or-lower generation is Stale.
        let permit_a = {
            let event = peer.next_event();
            let value = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
            let ShellContentRecord::FramePermit(p) = value.record else {
                panic!("frame permit");
            };
            peer.ack(&event);
            p
        };
        let a_bytes = encode_shell_file_candidate(
            candidate(ShellFileKind::Candidate, 1, 2),
            &ShellFileCandidate {
                transaction: TransactionId::from_raw(20),
                candidate: trivial_candidate(5, grant, output, permit_a.permit_id),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&a_bytes, 2);

        // B: the candidate under test, naming a stale (<=5) generation.
        // Granting its permit first supersedes A's still-pending slot:
        // `grant_permit` pushes A's Superseded outcome before its own
        // FramePermit, so that is the order the two arrive in.
        let event = peer.next_event();
        let value = decode_shell_file_transaction(&event, ShellFileKind::CandidateOutcome).unwrap();
        let ShellContentRecord::CandidateOutcome(outcome) = value.record else {
            panic!("candidate outcome");
        };
        assert_eq!((outcome.candidate_generation, outcome.kind), (5, 4));
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

        let b_bytes = encode_shell_file_candidate(
            candidate(ShellFileKind::Candidate, 1, 3),
            &ShellFileCandidate {
                transaction: TransactionId::from_raw(21),
                candidate: trivial_candidate(3, grant, output, permit_b.permit_id),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&b_bytes, 3);
        submitted_tx.send(()).unwrap();

        let event = peer.next_event();
        let value = decode_shell_file_transaction(&event, ShellFileKind::CandidateOutcome).unwrap();
        let ShellContentRecord::CandidateOutcome(outcome) = value.record else {
            panic!("candidate outcome");
        };
        assert_eq!(
            (outcome.candidate_generation, outcome.kind, outcome.reason),
            (3, 3, ContentReason::Stale as u16)
        );
        peer.ack(&event);

        // C: fresh permit, generation greater than any prior; proves the
        // connection survived B's rejection and nothing it discarded leaked.
        let permit_c = {
            let event = peer.next_event();
            let value = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
            let ShellContentRecord::FramePermit(p) = value.record else {
                panic!("frame permit");
            };
            peer.ack(&event);
            p
        };
        let c_bytes = encode_shell_file_candidate(
            candidate(ShellFileKind::Candidate, 1, 4),
            &ShellFileCandidate {
                transaction: TransactionId::from_raw(22),
                candidate: trivial_candidate(10, grant, output, permit_c.permit_id),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&c_bytes, 4);

        let event = peer.next_event();
        let value = decode_shell_file_transaction(&event, ShellFileKind::CandidateOutcome).unwrap();
        let ShellContentRecord::CandidateOutcome(outcome) = value.record else {
            panic!("candidate outcome");
        };
        assert_eq!((outcome.candidate_generation, outcome.kind), (10, 1));
        peer.ack(&event);
    });

    negotiate(&mut transport, &mut registry, 1, granted(), &peer)
        .unwrap()
        .expect("negotiated");
    let start = Instant::now();
    let context = ContentCandidateContext {
        output,
        facts_generation: 1,
        interaction_generation: 1,
        allocations: &[],
    };

    transport
        .grant_content_permit(&mut registry, TransactionId::from_raw(1), output, 1, 1, 0)
        .unwrap();
    let mut processed = 0;
    while processed < 3 {
        processed += transport
            .service_content_candidates(&mut registry, &[context], 1)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }

    transport
        .grant_content_permit(&mut registry, TransactionId::from_raw(2), output, 2, 2, 2)
        .unwrap();
    // Drive B: the fix means only its Begin is ever processed, never its
    // discarded Chunk/End, and the call keeps returning `Ok`.
    loop {
        let processed = transport
            .service_content_candidates(&mut registry, &[context], 3)
            .unwrap();
        if processed == 0 && submitted_rx.try_recv().is_ok() {
            while transport
                .service_content_candidates(&mut registry, &[context], 3)
                .unwrap()
                > 0
            {}
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }

    transport
        .grant_content_permit(&mut registry, TransactionId::from_raw(3), output, 3, 3, 4)
        .unwrap();
    let mut processed = 0;
    while processed < 3 {
        processed += transport
            .service_content_candidates(&mut registry, &[context], 5)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    let render = transport
        .begin_content_submission(&mut registry, output, 10, 6)
        .unwrap();
    transport
        .content_prepared(&mut registry, grant, output, 10, 1, 1, 7)
        .unwrap();
    drop(render);

    while !peer.is_finished() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}
