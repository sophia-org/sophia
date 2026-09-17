//! Ledger -> real transport FIFO -> private peer. No WM/native acceptance.
use super::*;
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

#[test]
fn expired_action_keeps_its_real_fifo_cancel_credit_until_exact_transfer() {
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
