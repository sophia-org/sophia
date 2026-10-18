mod support;

use std::os::fd::AsFd;

use sophia_image_producer::{
    ChannelState, CloseCause, ImageEvent, ImageRole, OfferState, ProducerError, ReadyEvents,
    ResourceStatusKind, create_isb,
};
use sophia_shell_protocol::shell_files::ShellImagePacket;
use support::{config, metadata, nothing_waiting, ready_producer, recv_packet, sealed_fd};

fn assert_source_packet(packet: &[u8], expect_offer: u64) {
    let decoded = ShellImagePacket::decode(packet).unwrap();
    let ShellImagePacket::Source {
        connection_epoch,
        channel_generation,
        metadata,
        ..
    } = decoded
    else {
        panic!("expected a Source packet");
    };
    assert_eq!(connection_epoch, 7);
    assert_eq!(channel_generation, 3);
    assert_eq!(metadata.offer_id, expect_offer);
    assert_eq!(metadata.resource, 20 + expect_offer);
    assert_eq!(metadata.resource_generation, 1);
    assert_eq!(metadata.width, 2);
    assert_eq!(metadata.height, 2);
    assert_eq!(metadata.stride, 8);
    assert_eq!(metadata.declared_bytes, 16);
}

fn fd_is_cloexec(fd: &std::os::fd::OwnedFd) -> bool {
    rustix::io::fcntl_getfd(fd.as_fd())
        .is_ok_and(|flags| flags.contains(rustix::io::FdFlags::CLOEXEC))
}

#[test]
fn admitted_source_crosses_once_with_a_cloexec_descriptor() {
    {
        let (mut producer, peer) = ready_producer(ImageRole::Shell);
        let (offer, body, assigned) = producer.begin(metadata(21), sealed_fd(), 4).unwrap();
        assert_eq!(offer, 1);
        assert_eq!(body.len(), 96);

        // Nothing crosses before admission.
        producer.dispatch(ReadyEvents::OUT, 5).unwrap();
        assert!(nothing_waiting(&peer));

        producer
            .record(7, ImageEvent::ImageAdmitted { metadata: assigned }, 6)
            .unwrap();
        producer.dispatch(ReadyEvents::OUT, 7).unwrap();
        let (packet, fds) = recv_packet(&peer);
        assert_eq!(packet.len(), 200);
        assert_source_packet(&packet, 1);
        assert_eq!(fds.len(), 1);
        assert!(fd_is_cloexec(&fds[0]));
        drop(fds);
        assert_eq!(producer.offer(1).unwrap().state, OfferState::Sent);

        // A second dispatch sends nothing again.
        producer.dispatch(ReadyEvents::OUT, 8).unwrap();
        assert!(nothing_waiting(&peer));

        producer
            .resource_status(21, 1, ResourceStatusKind::Accepted, false, 9)
            .unwrap();
        assert_eq!(producer.offer(1).unwrap().state, OfferState::Sent);
        producer
            .record(
                7,
                ImageEvent::SourceReleased {
                    offer_id: 1,
                    resource: 21,
                    resource_generation: 1,
                    disposition: 1,
                },
                10,
            )
            .unwrap();
        assert_eq!(producer.offer(1).unwrap().state, OfferState::Settled);
        producer.forget(1).unwrap();
    }
}

#[test]
fn backpressure_retains_the_admitted_source_until_drained() {
    let (mut producer, peer) = ready_producer(ImageRole::Shell);
    // Fill the channel's send buffer through the producer's own
    // socket until the kernel refuses more.
    let channel = producer.poll_fd().unwrap();
    let junk = [0u8; 200];
    let mut filled = 0u32;
    loop {
        match rustix::net::send(channel, &junk, rustix::net::SendFlags::DONTWAIT) {
            Ok(_) => filled += 1,
            Err(error) if error == rustix::io::Errno::AGAIN => break,
            Err(error) => panic!("fill failed: {error}"),
        }
        assert!(filled < 100_000);
    }
    assert!(filled > 0);

    let (_, _, assigned) = producer.begin(metadata(21), sealed_fd(), 10).unwrap();
    producer
        .record(7, ImageEvent::ImageAdmitted { metadata: assigned }, 11)
        .unwrap();
    producer.dispatch(ReadyEvents::OUT, 12).unwrap();
    // EAGAIN consumed nothing: still Admitted, descriptor retained.
    assert_eq!(producer.offer(1).unwrap().state, OfferState::Admitted);

    // Drain the junk, then the retained source goes out intact.
    while !nothing_waiting(&peer) {
        let _ = recv_packet(&peer);
    }
    // The caller is told to wait for writability again before the
    // resending dispatch.
    assert!(producer.poll_events().contains(ReadyEvents::OUT));
    let fd = producer.poll_fd().unwrap();
    let mut poll = [rustix::event::PollFd::new(
        &fd,
        rustix::event::PollFlags::OUT,
    )];
    assert_eq!(
        rustix::event::poll(&mut poll, Some(&rustix::event::Timespec::default())).unwrap(),
        1
    );
    assert!(poll[0].revents().contains(rustix::event::PollFlags::OUT));
    producer.dispatch(ReadyEvents::OUT, 13).unwrap();
    assert_eq!(producer.offer(1).unwrap().state, OfferState::Sent);
    let (packet, fds) = recv_packet(&peer);
    assert_source_packet(&packet, 1);
    assert_eq!(fds.len(), 1);
}

#[test]
fn dropped_receiver_makes_the_send_ambiguous_and_unreplayable() {
    let (mut producer, peer) = ready_producer(ImageRole::Shell);
    let (_, _, assigned) = producer.begin(metadata(21), sealed_fd(), 4).unwrap();
    producer
        .record(7, ImageEvent::ImageAdmitted { metadata: assigned }, 5)
        .unwrap();
    drop(peer);
    producer.dispatch(ReadyEvents::OUT, 6).unwrap();
    assert_eq!(producer.offer(1).unwrap().state, OfferState::Ambiguous);
    assert_eq!(producer.channel(), ChannelState::Closed);
    assert_eq!(producer.cause(), CloseCause::Unavailable);
    // No replay: custody and state are unchanged by later passes.
    producer.dispatch(ReadyEvents::OUT, 7).unwrap();
    assert_eq!(producer.offer(1).unwrap().state, OfferState::Ambiguous);
}

#[test]
fn stalled_admitted_offer_expires_without_losing_custody() {
    let (mut producer, peer) = ready_producer(ImageRole::Shell);
    let (_, _, assigned) = producer.begin(metadata(21), sealed_fd(), 4).unwrap();
    producer
        .record(7, ImageEvent::ImageAdmitted { metadata: assigned }, 5)
        .unwrap();
    assert_eq!(producer.timeout(5), Some(999));
    producer.dispatch(ReadyEvents::NONE, 1_005).unwrap();
    let info = producer.offer(1).unwrap();
    assert_eq!(info.state, OfferState::Expired);
    assert!(info.deadline_passed);
    assert!(nothing_waiting(&peer));
    // The release still settles an expired offer.
    producer
        .record(
            7,
            ImageEvent::SourceReleased {
                offer_id: 1,
                resource: 21,
                resource_generation: 1,
                disposition: 3,
            },
            1_006,
        )
        .unwrap();
    assert_eq!(producer.offer(1).unwrap().state, OfferState::Settled);
}

#[test]
fn four_offers_send_in_offer_order() {
    let (mut producer, peer) = ready_producer(ImageRole::Shell);
    let mut assigned = Vec::new();
    for resource in 21..=24u64 {
        let (offer, _, meta) = producer.begin(metadata(resource), sealed_fd(), 4).unwrap();
        assert_eq!(offer, resource - 20);
        assigned.push(meta);
    }
    // A fifth live offer exceeds the four slots.
    let (error, returned) = producer.begin(metadata(25), sealed_fd(), 4).unwrap_err();
    assert_eq!(error, ProducerError::Busy);
    drop(returned);
    // Admit in reverse; sends still follow offer order.
    for meta in assigned.iter().rev() {
        producer
            .record(7, ImageEvent::ImageAdmitted { metadata: *meta }, 5)
            .unwrap();
    }
    producer.dispatch(ReadyEvents::OUT, 6).unwrap();
    for expect in 1..=4u64 {
        let (packet, fds) = recv_packet(&peer);
        assert_source_packet(&packet, expect);
        assert_eq!(fds.len(), 1);
    }
    assert!(nothing_waiting(&peer));
}

#[test]
fn contradictory_release_identity_faults_and_keeps_custody() {
    let (mut producer, peer) = ready_producer(ImageRole::Shell);
    let (_, _, assigned) = producer.begin(metadata(21), sealed_fd(), 4).unwrap();
    producer
        .record(7, ImageEvent::ImageAdmitted { metadata: assigned }, 5)
        .unwrap();
    producer.dispatch(ReadyEvents::OUT, 6).unwrap();
    let _ = recv_packet(&peer);
    assert_eq!(
        producer.record(
            7,
            ImageEvent::SourceReleased {
                offer_id: 1,
                resource: 99,
                resource_generation: 1,
                disposition: 1,
            },
            7,
        ),
        Err(ProducerError::Closed)
    );
    assert_eq!(producer.channel(), ChannelState::Closed);
    assert_eq!(producer.cause(), CloseCause::Fault);
    assert_eq!(producer.offer(1).unwrap().state, OfferState::Sent);
}

#[test]
fn cancellation_before_a_late_admission_still_suppresses_the_send() {
    let (mut producer, peer) = ready_producer(ImageRole::Shell);
    let (_, _, assigned) = producer.begin(metadata(21), sealed_fd(), 4).unwrap();
    producer.cancel(1, 5).unwrap();
    producer
        .record(7, ImageEvent::ImageAdmitted { metadata: assigned }, 6)
        .unwrap();
    producer.dispatch(ReadyEvents::OUT, 7).unwrap();
    assert!(nothing_waiting(&peer));
    let info = producer.offer(1).unwrap();
    assert_eq!(info.state, OfferState::Admitted);
    assert!(info.cancel_requested);
}

#[test]
fn lock_role_runs_the_same_custody_flow() {
    let (mut producer, peer) = ready_producer(ImageRole::Lock);
    let (_, _, assigned) = producer.begin(metadata(21), sealed_fd(), 4).unwrap();
    producer
        .record(7, ImageEvent::ImageAdmitted { metadata: assigned }, 5)
        .unwrap();
    producer.dispatch(ReadyEvents::OUT, 6).unwrap();
    let (packet, fds) = recv_packet(&peer);
    assert_eq!(packet.len(), 200);
    // Lock role word in the packet header.
    assert_eq!(u16::from_le_bytes(packet[8..10].try_into().unwrap()), 2);
    assert_eq!(fds.len(), 1);
    producer
        .record(
            7,
            ImageEvent::SourceReleased {
                offer_id: 1,
                resource: 21,
                resource_generation: 1,
                disposition: 2,
            },
            7,
        )
        .unwrap();
    assert_eq!(producer.offer(1).unwrap().state, OfferState::Settled);
    let _ = config(ImageRole::Lock);
}

#[test]
fn unsealed_source_is_refused_in_ready_and_returned() {
    let (mut producer, _peer) = ready_producer(ImageRole::Shell);
    let unsealed = create_isb(2, 2).unwrap().into_fd();
    let (error, returned) = producer.begin(metadata(21), unsealed, 4).unwrap_err();
    assert_eq!(error, ProducerError::Refused);
    drop(returned);
    assert!(producer.offers().next().is_none());
}

#[test]
fn pre_admission_rejection_and_journal_loss_settle_as_documented() {
    let (mut producer, _peer) = ready_producer(ImageRole::Shell);
    producer.begin(metadata(21), sealed_fd(), 4).unwrap();
    producer
        .resource_status(21, 1, ResourceStatusKind::Rejected, false, 5)
        .unwrap();
    assert_eq!(producer.offer(1).unwrap().state, OfferState::Refused);
    producer.forget(1).unwrap();

    producer.begin(metadata(22), sealed_fd(), 6).unwrap();
    producer.journal_lost();
    assert_eq!(producer.offer(2).unwrap().state, OfferState::Unknown);
    assert_eq!(producer.channel(), ChannelState::Closed);
}
