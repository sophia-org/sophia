//! Supplied memfds exercise packet custody, not DMA-BUF import or GPU completion.
mod support;
use sophia_image_producer::*;
use sophia_shell_protocol::shell_files::*;
use std::os::fd::{AsFd, OwnedFd};
use support::*;

fn ready() -> (ImageProducer, OwnedFd) {
    ready_with_flags(SHELL_IMAGE_FORMAT_FLAG_OPAQUE)
}

fn ready_with_flags(flags: u32) -> (ImageProducer, OwnedFd) {
    let mut config = config(ImageRole::Shell);
    config.revision = 10;
    config.granted_kinds = 6;
    let mut producer = ImageProducer::new(config, 0).unwrap();
    let mut discovery = ShellImageDiscovery::decode(
        std::str::from_utf8(&discovery_text(ImageRole::Shell)).unwrap(),
    )
    .unwrap();
    discovery.source_kinds = 6;
    let mut limits = ShellImageLimits::decode(&limits_body(ImageRole::Shell, 9)).unwrap();
    limits.max_dma_source_bytes = 1 << 20;
    limits.max_renderer_allocation_bytes = 1 << 20;
    limits.max_renderer_entries = 4;
    limits.max_capture_jobs = 4;
    let mut formats = ShellImageFormats::decode(&formats_body(ImageRole::Shell)).unwrap();
    formats.rows.push(ShellImageFormatRow {
        output: 1,
        device_major: 226,
        device_minor: 128,
        fourcc: u32::from_le_bytes(*b"XR24"),
        flags,
        modifier: 0,
        max_width: 64,
        max_height: 64,
    });
    producer
        .discover(
            present(
                discovery.encode().unwrap().as_bytes(),
                &limits.encode().unwrap(),
                &formats.encode().unwrap(),
            ),
            1,
            0,
        )
        .unwrap();
    let (mine, peer) = seqpacket_pair();
    producer.attach(mine, 1).unwrap();
    producer.dispatch(ReadyEvents::OUT, 2).unwrap();
    assert_eq!(recv_packet(&peer).0.len(), 40);
    producer
        .record(
            7,
            ImageEvent::ChannelReady {
                channel_generation: 3,
                granted_source_kinds: 6,
            },
            3,
        )
        .unwrap();
    (producer, peer)
}

fn dma_metadata(resource: u64) -> ProducerMetadata {
    ProducerMetadata {
        source_kind: 2,
        source_format: u32::from_le_bytes(*b"XR24"),
        stride: 0,
        ..metadata(resource)
    }
}

#[test]
fn dma_source_waits_for_admission_and_exact_release() {
    let (mut producer, peer) = ready();
    let (offer, _, assigned) = producer
        .begin_dma_buf(dma_metadata(21), sealed_fd(), 0, 8, 4)
        .unwrap();
    producer.dispatch(ReadyEvents::OUT, 5).unwrap();
    assert!(nothing_waiting(&peer));
    producer
        .record(7, ImageEvent::ImageAdmitted { metadata: assigned }, 6)
        .unwrap();
    producer.dispatch(ReadyEvents::OUT, 7).unwrap();
    let (packet, fds) = recv_packet(&peer);
    assert_eq!(packet.len(), 200);
    assert_eq!(fds.len(), 1);
    let ShellImagePacket::Source {
        metadata, planes, ..
    } = ShellImagePacket::decode(&packet).unwrap()
    else {
        panic!("Source required")
    };
    assert_eq!(metadata.source_kind, 2);
    assert_eq!(metadata.stride, 0);
    assert_eq!(
        planes[0],
        ShellImagePlane {
            offset: 0,
            stride: 8,
            fd_index: 0
        }
    );
    producer.dispatch(ReadyEvents::OUT, 8).unwrap();
    assert!(nothing_waiting(&peer));
    producer
        .resource_status(21, 1, ResourceStatusKind::Accepted, false, 9)
        .unwrap();
    assert_eq!(producer.offer(offer).unwrap().state, OfferState::Sent);
    producer
        .record(
            7,
            ImageEvent::SourceReleased {
                offer_id: offer,
                resource: 21,
                resource_generation: 1,
                disposition: 2,
            },
            10,
        )
        .unwrap();
    assert_eq!(producer.offer(offer).unwrap().state, OfferState::Settled);
    producer.forget(offer).unwrap();
}

#[test]
fn local_layout_refusal_returns_fd_and_journal_loss_keeps_completion_unknown() {
    let (mut producer, _peer) = ready();
    for (offset, stride) in [(0, 7), (1, 8), (u32::MAX, u32::MAX)] {
        let (error, fd) = producer
            .begin_dma_buf(dma_metadata(21), sealed_fd(), offset, stride, 4)
            .unwrap_err();
        assert_eq!(error, ProducerError::Refused);
        assert!(rustix::io::fcntl_getfd(fd.as_fd()).is_ok());
    }
    let (offer, _, _) = producer
        .begin_dma_buf(dma_metadata(21), sealed_fd(), 0, 8, 5)
        .unwrap();
    producer.journal_lost();
    assert_eq!(producer.offer(offer).unwrap().state, OfferState::Unknown);
}

#[test]
fn padded_dma_allocation_is_distinct_from_logical_pixel_bytes() {
    let (mut producer, _) = ready();
    let allocation = create_isb(4, 2).unwrap();
    let fd = allocation.into_fd();
    assert!(
        producer
            .begin_dma_buf(dma_metadata(21), fd, 4, 16, 4)
            .is_ok()
    );
}

#[test]
fn first_phase_refuses_a_row_that_does_not_promise_opaque_capture() {
    let (mut producer, _) = ready_with_flags(SHELL_IMAGE_FORMAT_FLAG_VALIDATED);
    let (error, fd) = producer
        .begin_dma_buf(dma_metadata(21), sealed_fd(), 0, 8, 4)
        .unwrap_err();
    assert_eq!(error, ProducerError::Refused);
    assert!(rustix::io::fcntl_getfd(fd.as_fd()).is_ok());
}
