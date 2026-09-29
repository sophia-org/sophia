//! Rotating owner visits with two staged Negotiate submissions, one held at
//! a four-byte 9P frame prefix. Barriers establish readiness without sleeps.
use super::component_files::{Harness, evidence};
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::ShellContentAdmissionPolicy;
use sophia_session::shell_component_connections::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::shell_file_peer;

#[test]
fn bounded_negotiation_visits_both_peers_and_alternates_first_owner() {
    let mut h = Harness::new();
    let a = h.owner.reserve_attempt(0).unwrap();
    let b = h.owner.reserve_attempt(1).unwrap();
    let mut peers = Vec::new();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (sent_tx, sent_rx) = mpsc::channel();
    let (remainder_tx, remainder_rx) = mpsc::channel();
    let mut remainder_rx = Some(remainder_rx);
    for key in [a, b] {
        let native = key == b;
        h.owner
            .begin_negotiation(
                key,
                &evidence(),
                Duration::from_secs(5),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: native,
                },
            )
            .unwrap();
        let socket = h.owner.socket_path(key.slot).unwrap().to_owned();
        let ready_tx = ready_tx.clone();
        let sent_tx = sent_tx.clone();
        let (submit_tx, submit_rx) = mpsc::channel();
        let remainder = if native { None } else { remainder_rx.take() };
        let worker = std::thread::spawn(move || {
            let mut peer = shell_file_peer::Peer::connect(&socket);
            peer.setup();
            let hello = ShellV1ClientHello {
                minimum_revision: if native { 7 } else { 5 },
                maximum_revision: if native { 7 } else { 6 },
                required_capabilities: if native {
                    SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
                        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                        | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
                        | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER
                } else {
                    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                },
            };
            let offer = encode_shell_file_negotiate(
                ShellFileHeader {
                    kind: ShellFileKind::Negotiate,
                    connection_epoch: key.grant.connection_epoch,
                    submission_id: 1,
                    sequence: 0,
                },
                hello,
            )
            .unwrap();
            peer.open(5, b"transaction", 2);
            assert_eq!(peer.write(5, &offer).0, 119);
            ready_tx.send(key).unwrap();
            submit_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let submit = encode_shell_file_submit(ShellFileSubmit {
                connection_epoch: key.grant.connection_epoch,
                submission_id: 1,
                candidate_bytes: offer.len() as u32,
            })
            .unwrap();
            let body = [
                3u32.to_le_bytes().as_slice(),
                &0u64.to_le_bytes(),
                &(submit.len() as u32).to_le_bytes(),
                &submit,
            ]
            .concat();
            let prefix = if native { 7 + body.len() } else { 4 };
            assert_eq!(
                peer.rpc_with_prefix(118, &body, prefix, || {
                    sent_tx.send(key).unwrap();
                    if let Some(remainder) = remainder {
                        remainder.recv_timeout(Duration::from_secs(5)).unwrap();
                    }
                })
                .unwrap()
                .0,
                119
            );
            let submitted = peer.next_event();
            assert_eq!(
                decode_shell_file_submitted(&submitted)
                    .unwrap()
                    .submission_id,
                1
            );
            peer.ack(&submitted);
            peer.clear();
            let event = peer.next_event();
            let welcome = decode_shell_file_negotiated(&event).unwrap().welcome;
            assert_eq!(welcome.connection_epoch, key.grant.connection_epoch);
            assert_eq!(welcome.selected_revision, hello.maximum_revision);
            peer.ack(&event);
            peer.open(6, b"limits", 0);
            assert_eq!(
                decode_shell_file_limits(&peer.read(6, 0)).unwrap().grant,
                key.grant
            );
            peer
        });
        peers.push((key, submit_tx, worker));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ready = Vec::new();
    let mut visits = 0;
    while ready.len() != 2 {
        ready.extend(ready_rx.try_iter());
        assert!(h.owner.poll_negotiations(65536).iter().all(Option::is_none));
        visits += 1;
        assert!(Instant::now() < deadline, "peer setup hung");
        std::thread::yield_now();
    }
    assert!(ready.contains(&a) && ready.contains(&b));
    // Return the cursor to slot zero, then a zero-credit visit must rotate it.
    if visits % 2 != 0 {
        assert!(h.owner.poll_negotiations(0).iter().all(Option::is_none));
    }
    for (_, start, _) in &peers {
        start.send(()).unwrap();
    }
    let first = sent_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let second = sent_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_ne!(first, second);
    assert!(h.owner.poll_negotiations(0).iter().all(Option::is_none));
    assert_eq!(h.owner.phase(a), Ok(ComponentConnectionPhase::Negotiating));
    assert_eq!(h.owner.phase(b), Ok(ComponentConnectionPhase::Negotiating));
    let events = h.owner.poll_negotiations(65536);
    let (key, welcome) = events[0].as_ref().unwrap();
    assert_eq!(*key, b);
    assert_eq!(
        welcome.as_ref().unwrap().connection_epoch,
        b.grant.connection_epoch
    );
    assert!(events[1].is_none());
    assert_eq!(h.owner.phase(a), Ok(ComponentConnectionPhase::Negotiating));
    remainder_tx.send(()).unwrap();
    // The first poll may run before the writer has sent its remainder. Every
    // pending visit still rotates; verify the reported array position against
    // that exact cursor, rather than depending on the writer's scheduling.
    let mut first_slot = 0;
    loop {
        let events = h.owner.poll_negotiations(65536);
        if let Some((at, (key, result))) = events
            .into_iter()
            .enumerate()
            .find_map(|(i, e)| e.map(|e| (i, e)))
        {
            assert_eq!(key, a);
            assert_eq!(at, (a.slot + 2 - first_slot) % 2);
            result.unwrap();
            break;
        }
        first_slot = (first_slot + 1) % 2;
        assert!(Instant::now() < deadline, "fragmented negotiation hung");
        std::thread::yield_now();
    }
    while peers.iter().any(|(_, _, worker)| !worker.is_finished()) {
        for key in [a, b] {
            h.owner
                .with_connection(key, |t| t.poll_io().unwrap())
                .unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "negotiation acknowledgements hung"
        );
        std::thread::yield_now();
    }
    let clients: Vec<_> = peers
        .into_iter()
        .map(|(_, _, worker)| worker.join().unwrap())
        .collect();
    h.owner.close(a).unwrap();
    h.owner.close(b).unwrap();
    assert!(h.owner.collect().quiescent());
    drop(clients);
}
