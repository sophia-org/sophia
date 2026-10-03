//! Exercise owner/role service separation on a real 9P socket. The turn count
//! is work evidence; protocol replies, readiness and FIFO tests prove progress.
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn roles_share_input_but_new_publications_and_late_input_still_progress() {
    let directory = std::env::temp_dir().join(format!("shell-owner-turn-{}", std::process::id()));
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
    let mut epochs = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    transport
        .reserve_content(
            &mut epochs,
            ContentLimits::prototype(ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            }),
        )
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let (go, proceed) = mpsc::channel();
    let (signal, arrived) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: 1,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let event = peer.next_event();
        let offset = peer.event_offset();
        peer.ack(&event);
        signal.send(()).unwrap();
        proceed.recv_timeout(WAIT).unwrap();
        let body = [1u32.to_le_bytes().as_slice(), &0x7ffu64.to_le_bytes()].concat();
        assert_eq!(
            peer.rpc_with_prefix(24, &body, 7 + body.len(), || signal.send(()).unwrap())
                .unwrap()
                .0,
            25
        );
        signal.send(()).unwrap();
        proceed.recv_timeout(WAIT).unwrap();
        let body = [
            2u32.to_le_bytes().as_slice(),
            &offset.to_le_bytes(),
            &65500u32.to_le_bytes(),
        ]
        .concat();
        let (kind, bytes) = peer
            .rpc_with_prefix(116, &body, 7 + body.len(), || signal.send(()).unwrap())
            .unwrap();
        assert_eq!(kind, 117);
        let object = decode_shell_file_object_published(&bytes[4..]).unwrap();
        assert_eq!(object.object, ShellFileKind::Outputs);
        assert_eq!(object.generation, 1);
        signal.send(()).unwrap();
        proceed.recv_timeout(WAIT).unwrap();
    });
    transport
        .begin_file_negotiation(
            &epochs,
            1,
            WAIT,
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let started = Instant::now();
    while transport
        .poll_negotiation(&mut epochs, 64 * 1024)
        .unwrap()
        .is_none()
    {
        assert!(started.elapsed() < WAIT);
        std::thread::yield_now();
    }
    while arrived.try_recv().is_err() {
        transport.poll_io(&mut epochs).unwrap();
        assert!(started.elapsed() < WAIT);
        std::thread::yield_now();
    }

    let before = transport.wire_turn_count();
    transport.service_owner_turn(&mut epochs).unwrap();
    assert_eq!(transport.wire_turn_count(), before + 1);
    for _ in 0..40 {
        assert_eq!(
            transport
                .service_content_allocation_requests(&mut epochs, &[], 1)
                .unwrap(),
            0
        );
        assert!(
            transport
                .poll_content_action_ack(&mut epochs)
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        transport.wire_turn_count(),
        before + 1,
        "idle roles re-served the wire"
    );

    // A request arrives after the input pass. Role visits do not consume it;
    // readiness persists and the next owner pass replies without a timer.
    go.send(()).unwrap();
    arrived.recv_timeout(WAIT).unwrap();
    transport.poll_io(&mut epochs).unwrap();
    assert_eq!(transport.wire_turn_count(), before + 1);
    let mut fds = transport.poll_fds();
    assert!(rustix::event::poll(&mut fds, Some(&rustix::event::Timespec::default())).unwrap() > 0);
    drop(fds);
    transport.service_owner_turn(&mut epochs).unwrap();
    arrived.recv_timeout(WAIT).unwrap();

    // Register an empty journal read, then publish during the same owner pass.
    go.send(()).unwrap();
    arrived.recv_timeout(WAIT).unwrap();
    transport.service_owner_turn(&mut epochs).unwrap();
    let before = transport.wire_turn_count();
    transport
        .publish_content_output_facts(
            &mut epochs,
            TransactionId::from_raw(9),
            1,
            vec![ContentOutputFactsEntry {
                output: ContentOutputId {
                    id: 2,
                    generation: 1,
                },
                local_width: 64,
                local_height: 64,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
    transport.poll_io(&mut epochs).unwrap();
    assert_eq!(transport.wire_turn_count(), before + 1);
    arrived.recv_timeout(WAIT).unwrap();

    go.send(()).unwrap();
    peer.join().unwrap();
    transport.service_owner_turn(&mut epochs).unwrap();
    assert!(matches!(
        transport.poll_content_action_ack(&mut epochs),
        Err(ShellTransportError::NotConnected)
    ));
    transport.disconnect(&mut epochs).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}
