//! W2 prototype: the owner's retire-on-present setting reaches exactly the bar's
//! negotiation over a real socket. Session-owned configuration, supplied afid
//! authority; no Factotum or protected launch.
#![cfg(feature = "experimental-shell-retire-on-present")]
use sophia_config::ShellComponentRole;
use sophia_protocol::{shell_files::*, *};
use sophia_runtime::*;
use sophia_session::shell_component_connections::*;
use std::time::{Duration, Instant};
#[path = "../../sophia-runtime/tests/support/image_test_authority.rs"]
mod authority;
#[path = "../../sophia-runtime/tests/support/shell_file_peer.rs"]
mod raw;

const OWNED: u64 = 1 << 15;
const W2: u64 = SOPHIA_SHELL_CAPABILITY_CONTENT_RETIRE_ON_PRESENT;

fn attach(peer: &mut raw::Peer) {
    assert_eq!(
        peer.rpc(
            100,
            &[
                65536u32.to_le_bytes().as_slice(),
                &8u16.to_le_bytes(),
                b"9P2000.L"
            ]
            .concat()
        )
        .unwrap()
        .0,
        101
    );
    assert_eq!(
        peer.rpc(
            102,
            &[
                10u32.to_le_bytes().as_slice(),
                &[0; 4],
                &u32::MAX.to_le_bytes()
            ]
            .concat()
        )
        .unwrap()
        .0,
        103
    );
    assert_eq!(
        peer.rpc(
            104,
            &[
                1u32.to_le_bytes().as_slice(),
                &10u32.to_le_bytes(),
                &[0; 4],
                &u32::MAX.to_le_bytes()
            ]
            .concat()
        )
        .unwrap()
        .0,
        105
    );
    peer.set_revision(10);
    peer.open(2, b"events", 0);
    peer.open(3, b"submit", 1);
    peer.open(4, b"ack", 1);
}

/// One exact r12 bit-17 hello from `role` against an owner with the setting
/// `permitted`; the owner's negotiation result.
fn negotiate(
    permitted: bool,
    role: ShellComponentRole,
) -> Result<ShellV1ServerWelcome, ShellTransportError> {
    let directory = std::env::temp_dir().join(format!(
        "session-w2-{}-{permitted}-{role:?}",
        std::process::id()
    ));
    let mut owner = ShellComponentConnections::new().unwrap();
    owner.set_retire_on_present(permitted);
    let slot = owner
        .add(
            "component",
            role,
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
    let key = owner.reserve_attempt(slot).unwrap();
    owner
        .begin_authenticated_negotiation(
            key,
            &ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            },
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
            Box::new(authority::Factory),
        )
        .unwrap();
    let path = owner.socket_path(slot).unwrap().to_owned();
    let granted = permitted && role == ShellComponentRole::Bar;
    let (finish, finished) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut peer = raw::Peer::connect(&path);
        attach(&mut peer);
        let negotiate = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: key.grant.connection_epoch,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: 12,
                maximum_revision: 12,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                    | OWNED
                    | W2,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&negotiate, 1);
        let event = peer.next_event();
        if granted {
            let welcome = decode_shell_file_negotiated(&event).unwrap().welcome;
            assert_eq!(welcome.selected_revision, 12);
            assert_eq!(welcome.capabilities & W2, W2);
        } else {
            // The owner reports a refusal once the peer has read it.
            let record =
                decode_shell_file_record_at_revision(&event, ShellFileClass::Event, 10).unwrap();
            assert_eq!(record.header.kind, ShellFileKind::Refused);
        }
        peer.ack(&event);
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut outcome = None;
    // The peer's final ack needs service after the owner has its result.
    while outcome.is_none() || !peer.is_finished() {
        assert!(Instant::now() < deadline, "negotiation stalled");
        for (_, result) in owner.poll_negotiations(65536).into_iter().flatten() {
            outcome = Some(result);
            finish.send(()).unwrap();
        }
        let _ = owner.turn_connected();
        let mut fds = owner.poll_fds();
        rustix::event::poll(
            &mut fds,
            Some(&rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000,
            }),
        )
        .unwrap();
    }
    peer.join().unwrap();
    owner.close(key).unwrap();
    owner.finish_after_backend_drop(()).unwrap();
    outcome.unwrap()
}

#[test]
fn the_setting_permits_exactly_the_bar_to_negotiate_retire_on_present() {
    let welcome = negotiate(true, ShellComponentRole::Bar).unwrap();
    assert_eq!(welcome.selected_revision, 12);
    assert_eq!(welcome.capabilities & W2, W2);
    for (permitted, role) in [
        (false, ShellComponentRole::Bar),
        (true, ShellComponentRole::Dock),
    ] {
        let refused = negotiate(permitted, role);
        assert!(
            matches!(
                refused,
                Err(ShellTransportError::ContentAdmissionRefused(
                    ContentAdmissionRefused {
                        reason: 4,
                        denied_capabilities: W2,
                    }
                ))
            ),
            "{permitted} {role:?}: {refused:?}"
        );
    }
}
