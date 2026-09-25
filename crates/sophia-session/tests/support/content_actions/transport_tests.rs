//! Ledger -> real transport FIFO -> private peer. No WM/native acceptance.
use super::*;
#[path = "catalog_tests.rs"]
mod catalog_tests;
use sophia_protocol::*;
use sophia_runtime::ShellSessionTransport;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header).unwrap();
    let size = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    assert!(size <= 65536);
    let mut frame = header.to_vec();
    frame.resize(header.len() + size, 0);
    stream.read_exact(&mut frame[header.len()..]).unwrap();
    frame
}

fn transport_peer() -> (ShellSessionTransport, UnixStream, ContentLimits) {
    let directory = std::env::temp_dir().join(format!(
        "sophia-action-expiry-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut transport = ShellSessionTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    transport
        .authorize_protected_peer(&sophia_runtime::ProtectionDomainEvidence {
            backend: sophia_runtime::ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [sophia_runtime::ProtectionDomainRole::MetadataShell]
                .into_iter()
                .collect(),
        })
        .unwrap();
    let mut peer = UnixStream::connect(transport.socket_path()).unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    peer.set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    peer.write_all(
        &encode_shell_v1_client_hello_frame(ShellV1ClientHello {
            minimum_revision: 5,
            maximum_revision: 6,
            required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT,
        })
        .unwrap(),
    )
    .unwrap();
    transport
        .accept_and_negotiate_with_content_policy(
            1,
            Duration::from_secs(2),
            sophia_runtime::ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        )
        .unwrap();
    decode_shell_v1_server_welcome_frame(&read_frame(&mut peer)).unwrap();
    transport.poll_io().unwrap();
    let (_, ShellContentRecord::Limits(limits)) =
        decode_shell_content_frame(&read_frame(&mut peer)).unwrap()
    else {
        panic!("limits")
    };
    (transport, peer, limits)
}

#[test]
fn expired_action_keeps_its_real_fifo_cancel_credit_until_exact_transfer() {
    let (mut transport, mut peer, limits) = transport_peer();
    let directory = transport.socket_path().parent().unwrap().to_path_buf();
    let mut target = super::tests::target();
    target.grant = limits.grant;
    let mut ledger = ContentActionLedger::default();
    let event = ledger
        .issue(
            target,
            0,
            &limits,
            TransactionId::from_raw(1),
            &mut transport.connection(),
        )
        .unwrap()
        .unwrap();
    transport.poll_io().unwrap();
    let (_, ShellContentRecord::Action(action)) =
        decode_shell_content_frame(&read_frame(&mut peer)).unwrap()
    else {
        panic!("action")
    };
    assert_eq!((action.kind, action.event_id), (ACTION_ACTIVATE, event));
    // No acknowledgement. The actual ACK service must not discard the credit
    // at the deadline before the cancellation producer gets its turn.
    let now = u64::from(limits.action_ack_timeout_ms) + 1;
    assert_eq!(
        ledger
            .service_acks(&mut transport.connection(), now, 64)
            .unwrap(),
        0
    );
    let index = ledger
        .next_cancellation(&[], now)
        .expect("deadline retains cancel obligation");
    ledger
        .queue_cancellation(
            index,
            TransactionId::from_raw(2),
            &mut transport.connection(),
        )
        .unwrap();
    assert_eq!(ledger.next_cancellation(&[], now), None);
    transport.poll_io().unwrap();
    let (_, ShellContentRecord::Action(cancel)) =
        decode_shell_content_frame(&read_frame(&mut peer)).unwrap()
    else {
        panic!("cancel")
    };
    let mut expected = action;
    expected.kind = ACTION_CANCEL;
    assert_eq!(cancel, expected);
    assert!(ledger.live.is_empty());
    // Cancel has no ACK and cannot be emitted again on a later service turn.
    ledger
        .service_acks(&mut transport.connection(), now + 1, 64)
        .unwrap();
    assert_eq!(ledger.next_cancellation(&[], now + 1), None);
    peer.set_nonblocking(true).unwrap();
    let mut byte = [0];
    assert_eq!(
        peer.read(&mut byte).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    transport.disconnect().unwrap();
    drop(peer);
    drop(transport);
    assert!(
        !directory.exists(),
        "transport teardown removes its private endpoint"
    );
}

#[test]
fn a_full_action_queue_cannot_erase_the_outside_dismissal_deadline() {
    let (mut transport, mut peer, limits) = transport_peer();
    let mut target = super::tests::target();
    target.grant = limits.grant;
    let mut ledger = ContentActionLedger::default();
    for i in 0..limits.max_pending_actions {
        assert!(
            ledger
                .issue(
                    target.clone(),
                    1,
                    &limits,
                    TransactionId::from_raw(u64::from(i) + 1),
                    &mut transport.connection()
                )
                .unwrap()
                .is_some()
        );
    }
    let popout = sophia_engine::PresentedContentDismissal {
        grant: target.grant,
        output: target.output,
        candidate_generation: target.candidate_generation,
        presentation_epoch: target.presentation_epoch,
        interaction_generation: target.interaction_generation,
        allocation: target.allocation,
    };
    let deadline = 10 + u64::from(limits.action_ack_timeout_ms);
    for now in [10, 20] {
        assert_eq!(
            ledger
                .issue_dismissal(
                    popout.clone(),
                    now,
                    &limits,
                    TransactionId::from_raw(100),
                    &mut transport.connection()
                )
                .unwrap(),
            None
        );
    }
    assert!(ledger.dismissal_expired(popout.allocation, deadline));
    assert!(!ledger.dismissal_expired(popout.allocation, deadline - 1));
    assert_eq!(ledger.dismissals.len(), 1);
    assert!(!ledger.dismissals[0].notification_sent);
    assert_eq!(ledger.next_event_id, u64::from(limits.max_pending_actions) + 1);
    transport.poll_io().unwrap();
    for _ in 0..limits.max_pending_actions {
        let (_, ShellContentRecord::Action(action)) =
            decode_shell_content_frame(&read_frame(&mut peer)).unwrap()
        else {
            panic!("activation action");
        };
        assert_eq!(action.kind, 1);
    }
    peer.set_nonblocking(true).unwrap();
    assert_eq!(
        peer.read(&mut [0]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn dismissal_wire_identity_has_no_coordinates_and_ack_cannot_renew_its_deadline() {
    let (mut transport, mut peer, limits) = transport_peer();
    let target = super::tests::target();
    let popout = sophia_engine::PresentedContentDismissal {
        grant: limits.grant,
        output: target.output,
        candidate_generation: target.candidate_generation,
        presentation_epoch: target.presentation_epoch,
        interaction_generation: target.interaction_generation,
        allocation: target.allocation,
    };
    let mut ledger = ContentActionLedger::default();
    let id = ledger
        .issue_dismissal(
            popout.clone(),
            10,
            &limits,
            TransactionId::from_raw(1),
            &mut transport.connection(),
        )
        .unwrap()
        .unwrap();
    transport.poll_io().unwrap();
    let (_, ShellContentRecord::Action(action)) =
        decode_shell_content_frame(&read_frame(&mut peer)).unwrap()
    else {
        panic!("outside-dismiss action");
    };
    assert_eq!(
        (
            action.kind,
            action.target_id,
            action.target_generation,
            action.action_id
        ),
        (2, 0, 0, 0)
    );
    assert_eq!(
        (
            action.grant,
            action.output,
            action.allocation,
            action.presentation_epoch
        ),
        (
            popout.grant,
            popout.output,
            popout.allocation,
            popout.presentation_epoch
        )
    );
    let deadline = 10 + u64::from(limits.action_ack_timeout_ms);
    assert_eq!(
        ledger
            .issue_dismissal(
                popout,
                20,
                &limits,
                TransactionId::from_raw(2),
                &mut transport.connection()
            )
            .unwrap(),
        Some(id)
    );
    assert_eq!(ledger.dismissals.len(), 1);
    let mut ack = ContentActionAck {
        grant: action.grant,
        output: action.output,
        candidate_generation: action.candidate_generation,
        presentation_epoch: action.presentation_epoch,
        interaction_generation: action.interaction_generation,
        allocation: action.allocation,
        target_id: 0,
        target_generation: 0,
        action_id: 0,
        event_id: id,
        disposition: 1,
    };
    ack.presentation_epoch += 1;
    ledger.acknowledge(&ack, 21).unwrap();
    assert!(!ledger.dismissals[0].acknowledged);
    ack.presentation_epoch -= 1;
    peer.write_all(
        &encode_shell_content_frame(
            TransactionId::from_raw(3),
            &ShellContentRecord::ActionAck(ack),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        ledger
            .service_acks(&mut transport.connection(), 22, 1)
            .unwrap(),
        1
    );
    assert!(ledger.dismissals[0].acknowledged);
    assert!(!ledger.dismissal_expired(action.allocation, deadline - 1));
    assert!(ledger.dismissal_expired(action.allocation, deadline));
    assert_eq!(ledger.dismissals[0].deadline_msec, deadline);
}
