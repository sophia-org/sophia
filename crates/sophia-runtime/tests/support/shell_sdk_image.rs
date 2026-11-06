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
    let ring_slots = if client.welcome().selected_revision == 13 {
        2
    } else {
        1
    };
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
                ring_slots,
                ring_capacity: if ring_slots == 2 {
                    limits.max_resource_bytes
                } else {
                    0
                },
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
        let offered = if client.welcome().selected_revision == 13 {
            if client.image_adapter().unwrap().ring_identity().is_none() {
                client.poll_io().unwrap();
                assert!(Instant::now() < deadline, "SFT ring registration missing");
                continue;
            }
            match client.image_acquire_ring_fill(pixels.len(), Instant::now()) {
                Ok(mut fill) => {
                    fill.pixels_mut().unwrap().copy_from_slice(pixels);
                    let key = sophia_protocol::shell_files::ShellFrameStreamKey {
                        grant: sophia_protocol::ContentGrant {
                            connection_epoch: client.welcome().connection_epoch,
                            content_grant_epoch: client
                                .image_adapter()
                                .unwrap()
                                .ring_identity()
                                .unwrap()
                                .content_grant_epoch,
                        },
                        output: sophia_protocol::ContentOutputId {
                            id: metadata.output,
                            generation: metadata.output_generation,
                        },
                        allocation: sophia_protocol::ContentAllocationId {
                            id: metadata.allocation,
                            generation: metadata.allocation_generation,
                        },
                        facts_generation: metadata.topology_generation,
                        stream_id: 1,
                    };
                    Ok(FrameStream::new()
                        .commit_initial_ring_fill(client, key, &mut fill, metadata, Instant::now())
                        .unwrap())
                }
                Err(error) => Err(error),
            }
        } else {
            client.image_upload_ring(metadata, pixels, Instant::now())
        };
        match offered {
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
