mod support;

use sophia_image_producer::{ImageEvent, ImageRole, OfferState, ReadyEvents};
use support::{fd_count, metadata, ready_producer, recv_packet, sealed_fd};

/// Single test in this binary, so the process-wide descriptor
/// baseline is deterministic: every descriptor the flow creates
/// (pair, source, received right) is gone when the flow ends.
#[test]
fn descriptor_baseline_is_restored_after_a_full_flow() {
    let baseline = fd_count();
    {
        let (mut producer, peer) = ready_producer(ImageRole::Shell);
        let (_, _, assigned) = producer.begin(metadata(21), sealed_fd(), 4).unwrap();
        producer
            .record(7, ImageEvent::ImageAdmitted { metadata: assigned }, 5)
            .unwrap();
        producer.dispatch(ReadyEvents::OUT, 6).unwrap();
        let (packet, fds) = recv_packet(&peer);
        assert_eq!(packet.len(), 200);
        assert_eq!(fds.len(), 1);
        drop(fds);
        producer
            .record(
                7,
                ImageEvent::SourceReleased {
                    offer_id: 1,
                    resource: 21,
                    resource_generation: 1,
                    disposition: 1,
                },
                7,
            )
            .unwrap();
        assert_eq!(producer.offer(1).unwrap().state, OfferState::Settled);
        producer.forget(1).unwrap();
    }
    assert_eq!(fd_count(), baseline);
}
