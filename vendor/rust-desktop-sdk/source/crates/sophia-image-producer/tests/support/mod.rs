#![allow(dead_code)]

use std::io::IoSliceMut;
use std::os::fd::OwnedFd;

use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SocketFlags, SocketType,
    recvmsg, socketpair,
};
use sophia_image_producer::{
    DiscoveryRead, ImageProducer, ImageProducerConfig, ImageRole, ProducerMetadata,
    ProducerRoleBounds, ReadyEvents, create_isb, finish_isb,
};
use sophia_lock_protocol::lock_files::{LockImageDiscovery, LockImageFormats, LockImageLimits};
use sophia_shell_protocol::shell_files::{
    ShellImageDiscovery, ShellImageFormats, ShellImageLimits,
};

pub fn bounds() -> ProducerRoleBounds {
    ProducerRoleBounds {
        max_resource_bytes: 1 << 20,
        max_width: 64,
        max_height: 64,
        max_admitted_outputs: 1,
        transfer_slots: 4,
    }
}

pub fn config(role: ImageRole) -> ImageProducerConfig {
    ImageProducerConfig {
        role,
        revision: match role {
            ImageRole::Shell => 9,
            ImageRole::Lock => 2,
        },
        connection_epoch: 7,
        granted_kinds: 1,
        bounds: bounds(),
        transfer_timeout_ms: 1_000,
        startup_deadline_ms: 10_000,
        fallback_bytes: true,
    }
}

pub fn discovery_text(role: ImageRole) -> Vec<u8> {
    match role {
        ImageRole::Shell => ShellImageDiscovery {
            connection_epoch: 7,
            channel_generation: 3,
            endpoint: "img0".to_owned(),
            source_kinds: 1,
            max_offers: 4,
            max_packets_per_turn: 1,
            hello_timeout_ms: 250,
            validation_timeout_ms: 2_000,
            validation_chunk_bytes: 65_536,
            capture_interest_timeout_ms: 500,
            formats_generation: 11,
            limits_generation: 9,
        }
        .encode()
        .unwrap()
        .into_bytes(),
        ImageRole::Lock => LockImageDiscovery {
            connection_epoch: 7,
            channel_generation: 3,
            endpoint: "img0".to_owned(),
            source_kinds: 1,
            max_offers: 4,
            max_packets_per_turn: 1,
            hello_timeout_ms: 250,
            validation_timeout_ms: 2_000,
            validation_chunk_bytes: 65_536,
            capture_interest_timeout_ms: 500,
            formats_generation: 11,
            limits_generation: 9,
        }
        .encode()
        .unwrap()
        .into_bytes(),
    }
}

pub fn limits_body(role: ImageRole, limits_generation: u64) -> Vec<u8> {
    match role {
        ImageRole::Shell => ShellImageLimits {
            channel_generation: 3,
            limits_generation,
            max_resource_bytes: 1 << 20,
            max_sealed_backing_bytes: 1 << 20,
            max_dma_source_bytes: 0,
            max_renderer_allocation_bytes: 0,
            max_renderer_entries: 0,
            max_offers: 4,
            max_validation_jobs: 1,
            max_capture_jobs: 0,
            max_admitted_outputs: 1,
            max_format_rows_per_output: 1,
            max_width: 64,
            max_height: 64,
            validation_timeout_ms: 2_000,
            capture_interest_timeout_ms: 500,
            validation_chunk_bytes: 65_536,
            max_packets_per_turn: 1,
        }
        .encode()
        .unwrap(),
        ImageRole::Lock => LockImageLimits {
            channel_generation: 3,
            limits_generation,
            max_resource_bytes: 1 << 20,
            max_sealed_backing_bytes: 1 << 20,
            max_dma_source_bytes: 0,
            max_renderer_allocation_bytes: 0,
            max_renderer_entries: 0,
            max_offers: 4,
            max_validation_jobs: 1,
            max_capture_jobs: 0,
            max_admitted_outputs: 1,
            max_format_rows_per_output: 1,
            max_width: 64,
            max_height: 64,
            validation_timeout_ms: 2_000,
            capture_interest_timeout_ms: 500,
            validation_chunk_bytes: 65_536,
            max_packets_per_turn: 1,
        }
        .encode()
        .unwrap(),
    }
}

pub fn formats_body(role: ImageRole) -> Vec<u8> {
    match role {
        ImageRole::Shell => ShellImageFormats {
            channel_generation: 3,
            formats_generation: 11,
            limits_generation: 9,
            rows: Vec::new(),
        }
        .encode()
        .unwrap(),
        ImageRole::Lock => LockImageFormats {
            channel_generation: 3,
            formats_generation: 11,
            limits_generation: 9,
            rows: Vec::new(),
        }
        .encode()
        .unwrap(),
    }
}

pub fn present<'a>(text: &'a [u8], limits: &'a [u8], formats: &'a [u8]) -> DiscoveryRead<'a> {
    DiscoveryRead::Present {
        text,
        limits_body: limits,
        formats_body: formats,
        limits_connection_epoch: 7,
        formats_connection_epoch: 7,
    }
}

pub fn metadata(resource: u64) -> ProducerMetadata {
    ProducerMetadata {
        offer_id: 0,
        resource,
        resource_generation: 1,
        admission_epoch: 7,
        output_id: 1,
        allocation_id: 1,
        topology_generation: 1,
        source_kind: 1,
        source_format: 1,
        width: 2,
        height: 2,
        stride: 8,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 16,
        modifier: 0,
    }
}

pub fn sealed_fd() -> OwnedFd {
    let source = create_isb(2, 2).unwrap();
    source.write_pixels(0, &[7u8; 16]).unwrap();
    finish_isb(&source).unwrap();
    source.into_fd()
}

pub fn seqpacket_pair() -> (OwnedFd, OwnedFd) {
    socketpair(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None,
    )
    .unwrap()
}

/// A producer driven to Ready on a real SEQPACKET pair, with the
/// Hello already read and validated by the caller of this helper.
pub fn ready_producer(role: ImageRole) -> (ImageProducer, OwnedFd) {
    let text = discovery_text(role);
    let limits = limits_body(role, 9);
    let formats = formats_body(role);
    let mut producer = ImageProducer::new(config(role), 0).unwrap();
    producer
        .discover(present(&text, &limits, &formats), 1, 0)
        .unwrap();
    let (mine, peer) = seqpacket_pair();
    producer.attach(mine, 1).unwrap();
    producer.dispatch(ReadyEvents::OUT, 2).unwrap();
    let (hello, fds) = recv_packet(&peer);
    assert_eq!(hello.len(), 40);
    assert!(fds.is_empty());
    producer
        .record(
            7,
            sophia_image_producer::ImageEvent::ChannelReady {
                channel_generation: 3,
                granted_source_kinds: 1,
            },
            3,
        )
        .unwrap();
    (producer, peer)
}

/// Receive one packet and any descriptors that arrived with it.
pub fn recv_packet(peer: &OwnedFd) -> (Vec<u8>, Vec<OwnedFd>) {
    let mut buf = vec![0u8; 512];
    let mut space = [std::mem::MaybeUninit::uninit(); 128];
    let mut ancillary = RecvAncillaryBuffer::new(&mut space);
    let mut iov = [IoSliceMut::new(&mut buf)];
    let result = recvmsg(
        peer,
        &mut iov,
        &mut ancillary,
        RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC,
    )
    .unwrap();
    buf.truncate(result.bytes);
    let mut fds = Vec::new();
    for message in ancillary.drain() {
        if let RecvAncillaryMessage::ScmRights(rights) = message {
            fds.extend(rights);
        }
    }
    (buf, fds)
}

/// True when no packet is waiting on the peer.
pub fn nothing_waiting(peer: &OwnedFd) -> bool {
    let mut buf = [0u8; 1];
    let mut iov = [IoSliceMut::new(&mut buf)];
    let mut space = [std::mem::MaybeUninit::uninit(); 16];
    let mut ancillary = RecvAncillaryBuffer::new(&mut space);
    matches!(
        recvmsg(
            peer,
            &mut iov,
            &mut ancillary,
            RecvFlags::DONTWAIT | RecvFlags::PEEK,
        ),
        Err(error) if error == rustix::io::Errno::AGAIN
    )
}

pub fn fd_count() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}
