//! Bar (r6 view indicators) over the file wire: `indicators` does not exist
//! before negotiation or without bit 9, the `Indicators` object (pinned,
//! `EBUSY` for a second pin, a fresh qid on republish), and `IndicatorActivate`
//! to its outcome for both an accepted and a stale case (status/reason per
//! docs/sophia-shell-files.md "Role family outcomes (normative)").
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;

const MIB: u64 = 1024 * 1024;
const RLERROR: u8 = 7;
const EAGAIN: u32 = 11;
const ENOENT: u32 = 2;
const EBUSY: u32 = 16;
const EPOCH: u64 = 1;

const INDICATOR_CAPS: u64 =
    SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION;

fn transport() -> (ShellComponentTransport, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "shell-indicators-files-{}-{}",
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

fn header(kind: ShellFileKind, id: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: EPOCH,
        submission_id: id,
        sequence: 0,
    }
}

fn errno(reply: (u8, Vec<u8>)) -> u32 {
    assert_eq!(reply.0, RLERROR);
    u32::from_le_bytes(reply.1[..4].try_into().unwrap())
}

/// A raw walk from root for one name, without opening it: `Ok(())` on
/// success, `Err(errno)` on refusal (used for the ENOENT-before-disclosure
/// checks, where `Peer::open_path`'s own internal success assert cannot be
/// used).
fn walk_root(peer: &mut Peer, fid: u32, name: &[u8]) -> Result<(), u32> {
    let walk = [
        1u32.to_le_bytes().as_slice(),
        &fid.to_le_bytes(),
        &1u16.to_le_bytes(),
        &(name.len() as u16).to_le_bytes(),
        name,
    ]
    .concat();
    let reply = peer.rpc(110, &walk).unwrap();
    if reply.0 == RLERROR {
        Err(u32::from_le_bytes(reply.1[..4].try_into().unwrap()))
    } else {
        Ok(())
    }
}

fn negotiate(
    transport: &mut ShellComponentTransport,
    registry: &mut ContentEpochRegistry,
    peer_done: &std::thread::JoinHandle<()>,
) -> ShellV1ServerWelcome {
    transport
        .begin_file_negotiation(
            registry,
            EPOCH,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Unavailable,
        )
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
            label: "clock".to_owned(),
        }],
    }
}

/// `indicators` is absent from the root vocabulary before negotiation
/// completes at all, and stays absent for a bar connection that negotiates
/// without bit 9 -- unlike `outputs`/`limits`/`catalog`, which are walkable
/// before their object is ever published and only answer `EAGAIN`.
#[test]
fn indicators_do_not_exist_before_negotiation_or_without_bit_9() {
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
    let socket = transport.socket_path().to_owned();

    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();

        // Before any Negotiate record, nothing about this attach's
        // capabilities is known yet, so `indicators` cannot exist.
        assert_eq!(walk_root(&mut peer, 20, b"indicators"), Err(ENOENT));

        // Negotiate without requesting bit 9 at all.
        let offer = encode_shell_file_negotiate(
            header(ShellFileKind::Negotiate, 1),
            ShellV1ClientHello {
                minimum_revision: 1,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        let value = decode_shell_file_negotiated(&negotiated).unwrap();
        assert_eq!(
            value.welcome.capabilities & INDICATOR_CAPS,
            0,
            "bit 9 was not requested, so it and bit 10 must not be granted"
        );
        peer.ack(&negotiated);

        // Still absent: this profile's disclosure permission exists (it is
        // the bar), but the negotiated capability does not.
        assert_eq!(walk_root(&mut peer, 21, b"indicators"), Err(ENOENT));
    });

    negotiate(&mut transport, &mut registry, &peer);
    let start = Instant::now();
    while !peer.is_finished() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn indicators_object_publish_pin_republish_and_activate_cross_the_file_wire() {
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
    let socket = transport.socket_path().to_owned();

    let (checked_tx, checked_rx) = std::sync::mpsc::channel::<()>();
    let (pinned_tx, pinned_rx) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();

        let offer = encode_shell_file_negotiate(
            header(ShellFileKind::Negotiate, 1),
            ShellV1ClientHello {
                minimum_revision: 6,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | INDICATOR_CAPS,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        let value = decode_shell_file_negotiated(&negotiated).unwrap();
        assert_eq!(value.welcome.selected_revision, 6);
        assert_eq!(value.welcome.capabilities & INDICATOR_CAPS, INDICATOR_CAPS);
        peer.ack(&negotiated);

        // Now walkable, but nothing has been published yet.
        assert_eq!(errno(peer.open_path(20, &[b"indicators"], 0)), EAGAIN);
        checked_tx.send(()).unwrap();

        // The indicator snapshot object, pinned by an open fid.
        let published = peer.next_event();
        let first = decode_shell_file_object_published(&published).unwrap();
        assert_eq!(
            (first.object, first.generation),
            (ShellFileKind::Indicators, 5)
        );
        peer.ack(&published);
        peer.open(6, b"indicators", 0);
        let pinned = peer.read(6, 0);
        let value = decode_shell_file_indicators(&pinned).unwrap();
        assert_eq!(value.snapshot, snapshot(5));

        // One pin per feed per attach.
        peer.walk(7, b"indicators");
        let second = peer
            .rpc(12, &[7u32.to_le_bytes(), 0u32.to_le_bytes()].concat())
            .unwrap();
        assert_eq!(errno(second), EBUSY);
        pinned_tx.send(()).unwrap();

        // A republish gets a fresh qid even at a different generation; the
        // pinned fid keeps reading the old bytes.
        let republished = peer.next_event();
        let next = decode_shell_file_object_published(&republished).unwrap();
        assert_eq!(next.generation, 6);
        assert_ne!(next.qid, first.qid);
        peer.ack(&republished);
        assert_eq!(peer.read(6, 0), pinned);

        // After the pin is clunked, a new open sees the current object.
        assert_eq!(peer.rpc(120, &6u32.to_le_bytes()).unwrap().0, 121);
        peer.open(8, b"indicators", 0);
        let value = decode_shell_file_indicators(&peer.read(8, 0)).unwrap();
        assert_eq!(value.snapshot, snapshot(6));

        // An accepted activation.
        let activate = encode_shell_file_indicator_activate(
            header(ShellFileKind::IndicatorActivate, 2),
            &ShellFileIndicatorActivate {
                transaction: TransactionId::from_raw(20),
                activation: ShellIndicatorActivation {
                    connection_epoch: EPOCH,
                    snapshot_generation: 6,
                    output: OutputId::from_raw(3),
                    indicator: 1,
                    action: 2,
                    event_id: 1,
                },
            },
        )
        .unwrap();
        peer.submit_acknowledged(&activate, 2);
        let event = peer.next_event();
        let outcome = decode_shell_file_indicator_activation_outcome(&event).unwrap();
        assert_eq!(
            outcome.outcome.status,
            ShellIndicatorActivationStatus::Accepted
        );
        assert_eq!(outcome.outcome.reason, 0);
        assert_eq!(outcome.outcome.connection_epoch, EPOCH);
        assert_eq!(outcome.outcome.snapshot_generation, 6);
        assert_eq!(outcome.outcome.event_id, 1);
        peer.ack(&event);

        // A stale activation (a second Session-side decision, still using
        // the transport's typed queue exactly as the accepted one did).
        let stale_activate = encode_shell_file_indicator_activate(
            header(ShellFileKind::IndicatorActivate, 3),
            &ShellFileIndicatorActivate {
                transaction: TransactionId::from_raw(21),
                activation: ShellIndicatorActivation {
                    connection_epoch: EPOCH,
                    snapshot_generation: 1,
                    output: OutputId::from_raw(3),
                    indicator: 1,
                    action: 2,
                    event_id: 2,
                },
            },
        )
        .unwrap();
        peer.submit_acknowledged(&stale_activate, 3);
        let event = peer.next_event();
        let outcome = decode_shell_file_indicator_activation_outcome(&event).unwrap();
        assert_eq!(
            outcome.outcome.status,
            ShellIndicatorActivationStatus::Stale
        );
        assert_eq!(outcome.outcome.reason, 0);
        assert_eq!(outcome.outcome.event_id, 2);
        peer.ack(&event);
    });

    let start = Instant::now();
    negotiate(&mut transport, &mut registry, &peer);

    // Keep the file wire serviced for the peer's "not yet published" check
    // before the indicators object exists at all.
    while checked_rx.try_recv().is_err() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    transport
        .publish_indicators(&registry, TransactionId::from_raw(1), &snapshot(5))
        .unwrap();

    // Keep the file wire serviced until the peer has pinned the first
    // object (and proved the second-pin `EBUSY`) before superseding it: an
    // unpinned object can be dropped as soon as a fresher one replaces it,
    // so publishing again too early could race the peer's own open.
    while pinned_rx.try_recv().is_err() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    transport
        .publish_indicators(&registry, TransactionId::from_raw(2), &snapshot(6))
        .unwrap();

    let mut activations = 0;
    while activations < 2 {
        match transport.poll_indicator_activation(&mut registry) {
            Ok(Some((transaction, activation))) => {
                let status = if activations == 0 {
                    ShellIndicatorActivationStatus::Accepted
                } else {
                    ShellIndicatorActivationStatus::Stale
                };
                transport
                    .finish_indicator_activation(&mut registry, transaction, &activation, status, 0)
                    .unwrap();
                activations += 1;
            }
            Ok(None) => {}
            Err(ShellTransportError::NotConnected) if peer.is_finished() => break,
            Err(error) => panic!("poll_indicator_activation: {error:?}"),
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    assert_eq!(activations, 2);

    // The second (stale) outcome was recorded above but the file wire still
    // needs servicing to actually deliver it to the peer's pending read.
    while !peer.is_finished() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}
