//! Session-owned configuration and real sockets, with supplied afid authority.
//! This does not test Factotum or protected process launch.
use sophia_config::ShellComponentRole;
use sophia_protocol::{shell_files::*, *};
use sophia_runtime::*;
use sophia_session::shell_component_connections::*;
use std::time::{Duration, Instant};
#[path = "../../sophia-runtime/tests/support/image_test_authority.rs"]
mod authority;
#[path = "support/component_image_custody.rs"]
mod image_custody;
#[path = "../../sophia-runtime/tests/support/shell_file_peer.rs"]
mod raw;

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
    peer.open(2, b"events", 0);
    peer.open(3, b"submit", 1);
    peer.open(4, b"ack", 1);
}

#[test]
fn session_configures_each_authenticated_epoch_and_joins_the_image_socket() {
    let directory = std::env::temp_dir().join(format!("session-image-{}", std::process::id()));
    let mut owner = ShellComponentConnections::new().unwrap();
    let slot = owner
        .add(
            "bar",
            ShellComponentRole::Bar,
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
    for revision in [9, 6, 9] {
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
        let image_path = directory.join("image.sock");
        let (admitted, admission) = std::sync::mpsc::channel();
        let (close_source, await_close) = std::sync::mpsc::channel();
        let (change_topology, topology) = std::sync::mpsc::channel();
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
                    minimum_revision: revision,
                    maximum_revision: revision,
                    required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                        | if revision == 9 { 1 << 13 } else { 0 },
                },
            )
            .unwrap();
            peer.submit_acknowledged(&negotiate, 1);
            let event = peer.next_event();
            assert_eq!(
                decode_shell_file_negotiated(&event)
                    .unwrap()
                    .welcome
                    .selected_revision,
                revision
            );
            peer.ack(&event);
            if revision != 9 {
                let reply = peer
                    .rpc(
                        110,
                        &[
                            1u32.to_le_bytes().as_slice(),
                            &6u32.to_le_bytes(),
                            &1u16.to_le_bytes(),
                            &13u16.to_le_bytes(),
                            b"image-channel",
                        ]
                        .concat(),
                    )
                    .unwrap();
                assert_eq!(reply.0, 7);
                assert_eq!(u32::from_le_bytes(reply.1.try_into().unwrap()), 2);
                return;
            }
            peer.open(6, b"image-channel", 0);
            let discovery =
                ShellImageDiscovery::decode(std::str::from_utf8(&peer.read(6, 0)).unwrap())
                    .unwrap();
            assert_eq!(discovery.connection_epoch, key.grant.connection_epoch);
            assert_eq!(discovery.channel_generation, key.grant.connection_epoch);
            assert_eq!(discovery.source_kinds, 1);
            peer.open(7, b"image-limits", 0);
            let bytes = peer.read(7, 0);
            let record =
                decode_shell_file_record_at_revision(&bytes, ShellFileClass::Object, 9).unwrap();
            let limits = ShellImageLimits::decode(record.body).unwrap();
            let role = ContentLimits::prototype(key.grant);
            assert_eq!(limits.max_resource_bytes, role.max_resource_bytes);
            assert_eq!(
                limits.max_sealed_backing_bytes,
                role.max_resident_bytes + role.max_retiring_bytes
            );
            assert_eq!(limits.max_offers, role.max_open_transfers.min(4));
            let socket = rustix::net::socket_with(
                rustix::net::AddressFamily::UNIX,
                rustix::net::SocketType::SEQPACKET,
                rustix::net::SocketFlags::CLOEXEC,
                None,
            )
            .unwrap();
            rustix::net::connect(
                &socket,
                &rustix::net::SocketAddrUnix::new(image_path).unwrap(),
            )
            .unwrap();
            let hello = ShellImagePacket::Hello {
                connection_epoch: discovery.connection_epoch,
                channel_generation: discovery.channel_generation,
            }
            .encode()
            .unwrap();
            rustix::net::send(&socket, &hello, rustix::net::SendFlags::NOSIGNAL).unwrap();
            let event = image_custody::next_kind(&mut peer, ShellFileKind::ChannelReady);
            let record =
                decode_shell_file_record_at_revision(&event, ShellFileClass::Event, 9).unwrap();
            assert_eq!(
                ShellChannelReady::decode(record.body)
                    .unwrap()
                    .channel_generation,
                discovery.channel_generation
            );
            let ack = encode_shell_file_ack(ShellFileAck {
                connection_epoch: record.header.connection_epoch,
                sequence: record.header.sequence,
            })
            .unwrap();
            assert_eq!(peer.write(4, &ack).0, 119);
            peer.open(9, b"image-formats", 0);
            let old_formats = peer.read(9, 0);
            change_topology.send(()).unwrap();
            let changed = image_custody::next_kind(&mut peer, ShellFileKind::ObjectPublished);
            image_custody::ack(&mut peer, &changed);
            // The topology announcement follows both replacements; open fids
            // still read the exact bytes they pinned before that publication.
            assert_eq!(peer.read(9, 0), old_formats);
            assert_eq!(
                ShellImageDiscovery::decode(std::str::from_utf8(&peer.read(6, 0)).unwrap())
                    .unwrap(),
                discovery
            );
            peer.open(8, b"image-channel", 0);
            let current =
                ShellImageDiscovery::decode(std::str::from_utf8(&peer.read(8, 0)).unwrap())
                    .unwrap();
            assert_eq!(current.formats_generation, 2);
            assert_eq!(peer.rpc(120, &9u32.to_le_bytes()).unwrap().0, 121);
            peer.open(11, b"image-formats", 0);
            let bytes = peer.read(11, 0);
            let record =
                decode_shell_file_record_at_revision(&bytes, ShellFileClass::Object, 9).unwrap();
            assert_eq!(
                ShellImageFormats::decode(record.body)
                    .unwrap()
                    .formats_generation,
                2
            );
            image_custody::admit_unsent(&mut peer, key.grant);
            admitted.send(()).unwrap();
            await_close.recv_timeout(Duration::from_secs(2)).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut published = false;
        while !peer.is_finished() {
            assert!(Instant::now() < deadline);
            for (_, result) in owner.poll_negotiations(65536).into_iter().flatten() {
                result.unwrap();
            }
            assert!(owner.turn_connected().iter().all(Option::is_none));
            if topology.try_recv().is_ok() {
                owner
                    .with_connection(key, |transport| image_custody::publish(transport, 2))
                    .unwrap();
            }
            if revision == 9 && owner.phase(key) == Ok(ComponentConnectionPhase::Connected) {
                let now = owner.content_clock_msec();
                owner
                    .with_connection(key, |transport| {
                        image_custody::service(transport, now, &mut published);
                    })
                    .unwrap();
            }
            if admission.try_recv().is_ok() {
                assert_eq!(owner.accounting().memory.staging, 8);
                assert!(
                    owner.poll_fds().len() >= 3,
                    "worker retained beside the wire"
                );
                close_source.send(()).unwrap();
            }
            std::thread::yield_now();
        }
        peer.join().unwrap();
        owner.close(key).unwrap();
        assert!(owner.turn_connected().iter().all(Option::is_none));
        assert!(owner.poll_fds().is_empty());
        assert!(owner.collect().quiescent());
    }
    owner.finish_after_backend_drop(()).unwrap();
    drop(owner);
    assert!(
        !directory.exists(),
        "image listener drops before its endpoint"
    );
}
