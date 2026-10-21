//! Authenticated SDK image fixture. Authentication verdicts are supplied.
use sophia_protocol::ContentLimits;
use sophia_shell_client::*;
use std::time::{Duration, Instant};

// Supplied afid verdict, using the same authenticated attach seam as launch.
pub fn authenticated_client(
    path: &std::path::Path,
    options: ShellClientOptions,
) -> Result<ShellConnection, ShellClientError> {
    use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
    let deadline = Instant::now() + options.handshake_timeout;
    let mut pipeline =
        Pipeline::connect(path, PipelineLimits::default(), options.handshake_timeout).unwrap();
    let (tag, afid) = pipeline.auth(&[], &[]).unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline).unwrap(),
        Reply::Auth(_)
    ));
    let (tag, root) = pipeline.attach_with_afid(afid, &[], &[]).unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline).unwrap(),
        Reply::Attach(_)
    ));
    let tag = pipeline.clunk(afid).unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline).unwrap(),
        Reply::Clunk
    ));
    ShellConnection::from_attached(pipeline, root, options)
}

pub fn enable(client: &mut ShellConnection, directory: &std::path::Path, limits: &ContentLimits) {
    client
        .enable_image(
            ShellImageConfig {
                endpoint_dir: directory.to_owned(),
                bounds: ProducerRoleBounds {
                    max_resource_bytes: limits.max_resource_bytes,
                    max_width: limits.max_width_px,
                    max_height: limits.max_height_px,
                    max_admitted_outputs: limits.max_outputs,
                    transfer_slots: limits.max_open_transfers.min(4),
                },
                transfer_timeout_ms: limits.transfer_timeout_ms,
                startup_deadline_ms: 10000,
                formats_capacity: 65536,
                ring_slots: 1,
                ring_capacity: 0,
                content_grant_epoch: limits.grant.content_grant_epoch,
            },
            Instant::now(),
            0,
        )
        .unwrap();
}

pub fn upload(client: &mut ShellConnection, metadata: RingUpload, pixels: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while client.image_adapter().unwrap().channel() != Some(ImageChannelState::Ready) {
        client.poll_io().unwrap();
        assert!(Instant::now() < deadline, "image channel not ready");
        std::thread::yield_now();
    }
    let offer = loop {
        match client.image_upload_ring(metadata, pixels, Instant::now()) {
            Ok(offer) => break offer,
            Err(ImageAdapterError::Again | ImageAdapterError::Busy) => {
                client.poll_io().unwrap();
                assert!(Instant::now() < deadline, "ring not ready");
                std::thread::yield_now();
            }
            Err(error) => panic!("ring upload refused: {error:?}"),
        }
    };
    let mut accepted = false;
    loop {
        if let Some((_, record)) = client.poll_content().unwrap() {
            let sophia_protocol::ShellContentRecord::ResourceStatus(status) = record else {
                panic!("status: {record:?}")
            };
            assert_eq!(status.resource.id, metadata.resource);
            assert_eq!(status.status, 2, "{status:?}");
            accepted = true;
        }
        let info = client.image_ring_offer(offer).unwrap();
        if accepted && info.state == RingOfferState::Settled {
            break;
        }
        assert!(Instant::now() < deadline, "image not settled: {info:?}");
        std::thread::yield_now();
    }
    client.image_forget(offer).unwrap();
}
