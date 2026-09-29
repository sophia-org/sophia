//! Raw file records for Session services not exposed by the Rust SDK facade.
//! The 9P peer shares no runtime transport implementation with the server.
use super::component_files::{Harness, evidence};
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::{ShellContentAdmissionPolicy, ShellTransportConnection};
use sophia_session::shell_component_connections::ComponentConnectionKey;
use std::time::{Duration, Instant};

pub use super::shell_file_peer::Peer;

pub fn header(key: ComponentConnectionKey, kind: ShellFileKind, id: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: key.grant.connection_epoch,
        submission_id: id,
        sequence: 0,
    }
}

pub fn connect(h: &mut Harness, key: ComponentConnectionKey, native: bool) -> Peer {
    h.owner
        .begin_negotiation(
            key,
            &evidence(),
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: native,
            },
        )
        .unwrap();
    let socket = h.owner.socket_path(key.slot).unwrap().to_owned();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let hello = if native {
            ShellV1ClientHello {
                minimum_revision: 7,
                maximum_revision: 7,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
                    | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER,
            }
        } else {
            ShellV1ClientHello {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                    | SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS
                    | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION,
            }
        };
        peer.submit_acknowledged(
            &encode_shell_file_negotiate(header(key, ShellFileKind::Negotiate, 1), hello).unwrap(),
            1,
        );
        let event = peer.next_event();
        let negotiated = decode_shell_file_negotiated(&event).unwrap();
        assert_eq!(negotiated.welcome.selected_revision, hello.maximum_revision);
        assert_eq!(
            negotiated.welcome.capabilities & hello.required_capabilities,
            hello.required_capabilities
        );
        if native {
            assert_eq!(negotiated.welcome.capabilities, hello.required_capabilities);
        }
        peer.ack(&event);
        peer.open(6, b"limits", 0);
        let limits = decode_shell_file_limits(&peer.read(6, 0)).unwrap();
        (peer, negotiated.welcome, limits)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let welcome = loop {
        if let Some((received, result)) = h
            .owner
            .poll_negotiations(65536)
            .into_iter()
            .flatten()
            .next()
        {
            assert_eq!(received, key);
            break result.unwrap();
        }
        assert!(Instant::now() < deadline, "negotiation hung");
        std::thread::yield_now();
    };
    while !peer.is_finished() {
        h.owner
            .with_connection(key, |t| t.poll_io().unwrap())
            .unwrap();
        assert!(Instant::now() < deadline, "Limits fetch hung");
        std::thread::yield_now();
    }
    let (peer, received, limits) = peer.join().unwrap();
    assert_eq!(received, welcome);
    assert_eq!(welcome.connection_epoch, key.grant.connection_epoch);
    assert_eq!(limits.grant, key.grant);
    h.owner
        .with_connection(key, |t| assert_eq!(t.content_limits(), Some(&limits)))
        .unwrap();
    peer
}

pub fn drive<R: Send>(
    h: &mut Harness,
    key: ComponentConnectionKey,
    peer: &mut Peer,
    operation: impl FnOnce(&mut Peer) -> R + Send,
    mut service: impl FnMut(&mut ShellTransportConnection<'_>),
) -> R {
    std::thread::scope(|scope| {
        let operation = scope.spawn(move || operation(peer));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !operation.is_finished() {
            h.owner
                .with_connection(key, |t| {
                    service(t);
                    t.poll_io().unwrap();
                })
                .unwrap();
            assert!(Instant::now() < deadline, "file operation hung");
            std::thread::yield_now();
        }
        operation.join().unwrap()
    })
}
