#![allow(dead_code)]
//! Supplied memfds test r10 adapter custody, not GPU import or completion.
#[path = "support/file_wire_peer.rs"]
mod file_wire_peer;
#[path = "support/image_adapter_peer.rs"]
mod image_peer;
use file_wire_peer::{CONTENT_GRANT_EPOCH, EPOCH, Peer, Profile};
use image_peer::*;
use sophia_shell_client::*;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::time::Instant;

fn metadata() -> ProducerMetadata {
    let mut metadata = image_peer::metadata();
    metadata.source_kind = 2;
    metadata.source_format = u32::from_le_bytes(*b"XR24");
    metadata.stride = 0;
    metadata.admission_epoch = CONTENT_GRANT_EPOCH;
    metadata
}

fn configure_dma_discovery(peer: &mut Peer) {
    let object = peer.objects.get_mut("image-channel").unwrap();
    let mut discovery =
        ShellImageDiscovery::decode(std::str::from_utf8(&object.bytes).unwrap()).unwrap();
    discovery.source_kinds = 6;
    object.bytes = discovery.encode().unwrap().into_bytes();
    let object = peer.objects.get_mut("image-limits").unwrap();
    let record =
        decode_shell_file_record_at_revision(&object.bytes, ShellFileClass::Object, 10).unwrap();
    let mut limits = ShellImageLimits::decode(record.body).unwrap();
    limits.max_dma_source_bytes = 1 << 20;
    limits.max_renderer_allocation_bytes = 1 << 20;
    limits.max_renderer_entries = 4;
    limits.max_capture_jobs = 4;
    object.bytes =
        encode_shell_file_record_at_revision(record.header, &limits.encode().unwrap(), 10).unwrap();
    let object = peer.objects.get_mut("image-formats").unwrap();
    let record =
        decode_shell_file_record_at_revision(&object.bytes, ShellFileClass::Object, 10).unwrap();
    let mut formats = ShellImageFormats::decode(record.body).unwrap();
    formats.rows.push(ShellImageFormatRow {
        output: 1,
        device_major: 226,
        device_minor: 128,
        fourcc: u32::from_le_bytes(*b"XR24"),
        flags: SHELL_IMAGE_FORMAT_FLAG_OPAQUE,
        modifier: 0,
        max_width: 64,
        max_height: 64,
    });
    object.bytes =
        encode_shell_file_record_at_revision(record.header, &formats.encode().unwrap(), 10)
            .unwrap();
}

#[test]
fn dma_adapter_waits_for_admission_and_completion_release() {
    let mut profile = profile();
    profile.capabilities |= IMAGE_CAPABILITY_DMA_BUF;
    let (connection, mut peer, dir, listener) =
        connect_role_image("dma-flow", 4096, profile, false);
    configure_dma_discovery(&mut peer);
    complete_offer((connection, peer, dir, listener), false);
}

#[test]
fn ring_client_accepts_dma_discovery_with_cpu_only_channel_ready() {
    let (mut connection, mut peer, dir, listener) = connect_image("dma-discovery-ring");
    configure_dma_discovery(&mut peer);
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let _ = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    assert_eq!(
        connection.image_adapter().unwrap().channel(),
        Some(ImageChannelState::Ready)
    );
    drop(connection);
    drop(peer);
    drop(accepted);
    drop(listener);
    std::fs::remove_dir_all(dir).unwrap();
}

fn answer_submit(connection: &mut ShellConnection, peer: &mut Peer, kind: ShellFileKind) {
    drive(connection, peer, |_, peer| !peer.submits.is_empty());
    let held = peer.submits.pop_front().unwrap();
    let record = peer.transactions.last().unwrap();
    let submission_id = u64::from_le_bytes(record[16..24].try_into().unwrap());
    peer.answer_write(held.tag, held.data.len() as u32);
    let submitted = peer.submitted_image(submission_id, kind);
    peer.push(submitted);
}
fn complete_offer(
    (mut connection, mut peer, dir, listener): (ShellConnection, Peer, PathBuf, OwnedFd),
    native: bool,
) {
    let mut lifecycle = if native {
        let ShellContentRecord::Limits(limits) = take_role_content(&mut connection) else {
            panic!("Limits required")
        };
        Some(NativeLauncherLifecycle::new(limits).unwrap())
    } else {
        None
    };
    // Discovery over 9P, endpoint connect, Hello on the side socket.
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_poll_fd().is_some()
    });
    let accepted = accept_endpoint(&listener);
    let (hello, hello_fds) = recv_packet(&accepted);
    assert_eq!(hello.len(), 40);
    assert!(hello_fds.is_empty());
    assert_eq!(
        connection.image_adapter().unwrap().channel(),
        Some(ImageChannelState::HelloSent)
    );

    // ChannelReady is consumed by the session, never delivered.
    let ready = ShellChannelReady {
        channel_generation: 3,
        granted_source_kinds: 6,
    }
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::ChannelReady, &ready);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Ready)
    });
    assert_eq!(connection.take_content().unwrap(), None);

    // Begin: the ImageBegin record goes through the ordinary
    // transaction/submit custody cycle with its own ticket.
    let admission = connection
        .image_begin_dma_buf(metadata(), sealed_fd(), 0, 8, Instant::now())
        .unwrap();
    assert_eq!(admission.offer, 1);
    assert_eq!(
        connection
            .image_adapter()
            .unwrap()
            .dma_offer(1)
            .unwrap()
            .state,
        OfferState::Begun
    );
    let mut packet = [0u8; 1];
    assert_eq!(
        rustix::net::recv(&accepted, &mut packet, rustix::net::RecvFlags::DONTWAIT),
        Err(rustix::io::Errno::AGAIN)
    );
    answer_submit(&mut connection, &mut peer, ShellFileKind::ImageBegin);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.custody(admission.ticket) == Some(Custody::Submitted)
    });
    let record = peer.transactions.last().unwrap().clone();
    assert_eq!(record.len(), 128);
    assert_eq!(
        u16::from_le_bytes(record[6..8].try_into().unwrap()),
        ShellFileKind::ImageBegin as u16
    );

    // Admission, then exactly one Source with one descriptor.
    let mut admitted = metadata();
    admitted.offer_id = 1;
    let body = ShellImageMetadata {
        offer_id: 1,
        resource: 21,
        resource_generation: 1,
        admission_epoch: CONTENT_GRANT_EPOCH,
        output_id: 1,
        allocation_id: 1,
        topology_generation: 1,
        source_kind: 2,
        source_format: u32::from_le_bytes(*b"XR24"),
        width: 2,
        height: 2,
        stride: 0,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 16,
        modifier: 0,
    }
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::ImageAdmitted, &body);
    peer.push(bytes);
    let _ = admitted;
    drive(&mut connection, &mut peer, |connection, _| {
        connection
            .image_adapter()
            .unwrap()
            .dma_offer(1)
            .unwrap()
            .state
            == OfferState::Sent
    });
    let (packet, fds) = recv_packet(&accepted);
    assert_eq!(packet.len(), 200);
    assert_eq!(fds.len(), 1);
    let decoded = ShellImagePacket::decode(&packet).unwrap();
    assert!(matches!(decoded, ShellImagePacket::Source { .. }));
    assert_eq!(
        connection
            .image_adapter()
            .unwrap()
            .dma_offer(1)
            .unwrap()
            .state,
        OfferState::Sent
    );

    // ResourceStatus is routed to the producer and still delivered.
    let status_body = encode_shell_file_resource_status_body(&ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(1),
        record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant: ContentGrant {
                connection_epoch: EPOCH,
                content_grant_epoch: CONTENT_GRANT_EPOCH,
            },
            resource: ContentResourceId {
                id: 21,
                generation: 1,
            },
            status: 2,
            reason: 0,
            next_ordinal: 0,
            admitted_bytes: 16,
        }),
    })
    .unwrap();
    let bytes = peer.event(ShellFileKind::ResourceStatus, &status_body);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection
            .image_adapter()
            .unwrap()
            .dma_offer(1)
            .unwrap()
            .status
            == Some(sophia_image_producer::ResourceStatusKind::Accepted)
    });
    assert!(matches!(
        take_role_content(&mut connection),
        ShellContentRecord::ResourceStatus(_)
    ));
    assert_eq!(
        connection
            .image_adapter()
            .unwrap()
            .dma_offer(1)
            .unwrap()
            .state,
        OfferState::Sent,
        "a status records the outcome but does not settle custody"
    );

    if let Some(lifecycle) = &mut lifecycle {
        assert!(connection.adopt_native_image(lifecycle, 1).is_err());
    }
    // Only SourceReleased settles.
    let released = ShellSourceReleased {
        offer_id: 1,
        resource: 21,
        resource_generation: 1,
        disposition: ShellSourceDisposition::CaptureComplete,
    }
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::SourceReleased, &released);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection
            .image_adapter()
            .unwrap()
            .dma_offer(1)
            .unwrap()
            .state
            == OfferState::Settled
    });
    if let Some(lifecycle) = &mut lifecycle {
        assert_eq!(
            connection.adopt_native_image(lifecycle, 1).unwrap(),
            ContentResourceId {
                id: 21,
                generation: 1
            }
        );
        assert!(connection.adopt_native_image(lifecycle, 1).is_err());
    } else {
        connection.image_forget(1).unwrap();
    }
    assert!(connection.image_adapter().unwrap().dma_offer(1).is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn native_dma_adoption_requires_capture_complete_and_consumes_offer_once() {
    let profile = Profile {
        role: "launcher",
        revision: 10,
        capabilities: 0x9a0 | OWNED | IMAGE_CAPABILITY_DMA_BUF,
        limits_published: true,
    };
    let (connection, mut peer, dir, listener) =
        connect_role_image("dma-native", 4096, profile, false);
    configure_dma_discovery(&mut peer);
    complete_offer((connection, peer, dir, listener), true);
}
