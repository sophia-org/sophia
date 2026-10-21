//! In-process smoke: the actual ring peer against a real component
//! transport, owner and worker — three frames over two slots, no launch.
use super::*;
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits};
use sophia_runtime::{
    ContentAllocationSnapshot, ContentEpochRegistry, ProtectionBackendKind,
    ProtectionDomainEvidence, ProtectionDomainRole, ShellComponentTransport,
    ShellContentAdmissionPolicy, ShellIsbImageOwner, ShellTransportError,
};
use std::time::Instant;

#[path = "../../tests/support/image_test_authority.rs"]
mod authority;

const GRANT: ContentGrant = ContentGrant {
    connection_epoch: 1,
    content_grant_epoch: 1,
};

fn image_config() -> (
    ShellImageDiscovery,
    ShellImageLimits,
    ShellImageFormats,
    ShellImageRoleBounds,
) {
    (
        ShellImageDiscovery {
            connection_epoch: 1,
            channel_generation: 1,
            endpoint: "image.sock".into(),
            source_kinds: 1,
            max_offers: 4,
            max_packets_per_turn: 1,
            hello_timeout_ms: 250,
            validation_timeout_ms: 2000,
            validation_chunk_bytes: 65536,
            capture_interest_timeout_ms: 500,
            formats_generation: 1,
            limits_generation: 1,
        },
        ShellImageLimits {
            channel_generation: 1,
            limits_generation: 1,
            max_resource_bytes: 1 << 20,
            max_sealed_backing_bytes: 4 << 20,
            max_dma_source_bytes: 0,
            max_renderer_allocation_bytes: 0,
            max_renderer_entries: 0,
            max_offers: 4,
            max_validation_jobs: 4,
            max_capture_jobs: 0,
            max_admitted_outputs: 1,
            max_format_rows_per_output: 256,
            max_width: 1920,
            max_height: 1024,
            validation_timeout_ms: 2000,
            capture_interest_timeout_ms: 500,
            validation_chunk_bytes: 65536,
            max_packets_per_turn: 1,
        },
        ShellImageFormats {
            channel_generation: 1,
            formats_generation: 1,
            limits_generation: 1,
            rows: vec![],
        },
        ShellImageRoleBounds {
            max_resource_bytes: 1 << 20,
            max_width: 1920,
            max_height: 1024,
            max_admitted_outputs: 1,
            transfer_slots: 4,
        },
    )
}

#[test]
fn ring_peer_against_real_transport() {
    run_smoke(Config {
        output_id: 1,
        frames: 3,
        height: 20,
        hold: 0,
        source: Source::Ring { slots: 2 },
    });
}

#[test]
fn sealed_peer_against_real_transport() {
    let limits = ContentLimits::prototype(GRANT);
    run_smoke(Config {
        output_id: 1,
        frames: 2,
        height: 20,
        hold: 0,
        source: Source::Sealed {
            bounds: SealedBounds {
                max_resource_bytes: limits.max_resource_bytes,
                max_width: limits.max_width_px,
                max_height: limits.max_height_px,
                max_admitted_outputs: limits.max_outputs,
                transfer_slots: limits.max_open_transfers,
                transfer_timeout_ms: limits.transfer_timeout_ms,
            },
        },
    });
}

fn run_smoke(config: Config) {
    let frames = config.frames;
    let directory = std::env::temp_dir().join(format!(
        "shell-owned-ring-smoke-{}-{}",
        std::process::id(),
        match config.source {
            Source::Ring { slots } => format!("ring{slots}"),
            Source::Sealed { .. } => "sealed".to_string(),
        }
    ));
    let _ = std::fs::remove_dir_all(&directory);
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    transport
        .authorize_authenticated_peer(
            &ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            },
            Box::new(authority::Factory),
        )
        .unwrap();
    let (d, l, f, b) = image_config();
    transport.configure_image_channel(d, l, f, b).unwrap();
    let mut epochs = ContentEpochRegistry::new(64 << 20).unwrap();
    transport
        .reserve_content(&mut epochs, ContentLimits::prototype(GRANT))
        .unwrap();
    // Output facts are published through the transport below, once the
    // file negotiation exists to carry them.
    transport
        .begin_file_negotiation(
            &epochs,
            1,
            Duration::from_secs(3),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let path = transport.socket_path().to_owned();
    let peer_thread = std::thread::spawn(move || {
        let mut pipe = Pipeline::connect(&path, PipelineLimits::default(), WAIT).unwrap();
        let (tag, afid) = pipe.auth(b"", b"").unwrap();
        assert!(matches!(
            pipe.wait(tag, Instant::now() + WAIT).unwrap(),
            sophia_9p_client::pipeline::Reply::Auth(_)
        ));
        let (tag, root) = pipe.attach_with_afid(afid, b"", b"").unwrap();
        assert!(matches!(
            pipe.wait(tag, Instant::now() + WAIT).unwrap(),
            sophia_9p_client::pipeline::Reply::Attach(_)
        ));
        let mut client = Client::new(pipe, root);
        client.init().unwrap();
        serve(&mut client, &config, path.as_os_str()).unwrap();
    });
    let snapshot = ContentAllocationSnapshot {
        native_opening: None,
        output: ContentOutputId {
            id: 1,
            generation: 1,
        },
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        scale_generation: 1,
        scale_numerator: 1,
        scale_denominator: 1,
        role: 1,
        edge: 1,
        margins: ContentMargins::default(),
        logical: ContentLogicalRect {
            x: 0,
            y: 0,
            width: 200,
            height: 20,
        },
        pixel: ContentPixelRect {
            x: 0,
            y: 0,
            width: 200,
            height: 20,
        },
        parent: ContentAllocationId::default(),
        anchor_parent_rect: ContentPixelRect::default(),
        allowed_reservation_extent: 20,
    };
    let mut owner = ShellIsbImageOwner::new(4 << 20, 65536).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut negotiated = false;
    let mut facts_published = false;
    let mut granted = false;
    while !peer_thread.is_finished() {
        assert!(Instant::now() < deadline, "ring smoke stalled");
        if !negotiated {
            negotiated = transport
                .poll_negotiation(&mut epochs, 65536)
                .unwrap()
                .is_some();
        } else {
            match transport.service_owner_turn(&mut epochs) {
                Ok(()) | Err(ShellTransportError::NotConnected) => {}
                Err(error) => panic!("owner turn: {error}"),
            }
            match transport.service_content_resources(&mut epochs, 0) {
                Ok(_) | Err(ShellTransportError::NotConnected) => {}
                Err(error) => panic!("resource service: {error}"),
            }
            match transport.service_content_allocation_requests(&mut epochs, &[], 0) {
                Ok(_) | Err(ShellTransportError::NotConnected) => {}
                Err(error) => panic!("allocation service: {error}"),
            }
            match transport.service_content_demands(
                &mut epochs,
                &[ContentOutputId {
                    id: 1,
                    generation: 1,
                }],
                std::slice::from_ref(&snapshot),
            ) {
                Ok(_) | Err(ShellTransportError::NotConnected) => {}
                Err(error) => panic!("demand service: {error}"),
            }
            if let Some((tx, demand)) = transport.next_content_demand(&epochs) {
                match transport.grant_content_demand(
                    &mut epochs,
                    tx,
                    demand.output,
                    demand.demand_id,
                    0,
                ) {
                    Ok(()) | Err(ShellTransportError::NotConnected) => {}
                    Err(error) => panic!("permit grant: {error}"),
                }
            }
            match transport.service_content_candidates(
                &mut epochs,
                &[sophia_runtime::ContentCandidateContext {
                    output: ContentOutputId {
                        id: 1,
                        generation: 1,
                    },
                    facts_generation: 1,
                    interaction_generation: 1,
                    allocations: std::slice::from_ref(&snapshot),
                }],
                0,
            ) {
                Ok(_) | Err(ShellTransportError::NotConnected) => {}
                Err(error) => panic!("candidate service: {error}"),
            }
            if let Some((output, generation)) = transport.next_content_submission(&epochs) {
                transport
                    .begin_content_submission(&mut epochs, output, generation, 0)
                    .unwrap();
                transport
                    .content_prepared(&mut epochs, GRANT, output, generation, 1, 1, 0)
                    .unwrap();
                transport
                    .content_presented(&mut epochs, GRANT, output, generation, generation, 1, 1)
                    .unwrap();
            }
            match owner.service(&mut transport, &mut epochs, 0, Instant::now()) {
                Ok(()) | Err(ShellTransportError::NotConnected) => {}
                Err(error) => panic!("owner service: {error}"),
            }
            if !facts_published {
                let attempt = transport.publish_content_output_facts(
                    &mut epochs,
                    TransactionId::from_raw(1),
                    1,
                    vec![ContentOutputFactsEntry {
                        output: ContentOutputId {
                            id: 1,
                            generation: 1,
                        },
                        local_width: 200,
                        local_height: 100,
                        scale_numerator: 1,
                        scale_denominator: 1,
                        scale_generation: 1,
                    }],
                );
                facts_published = attempt.is_ok();
            }
            if facts_published
                && !granted
                && let Some((_, request)) = transport.next_content_allocation_request(&epochs)
            {
                transport
                    .grant_content_allocation(
                        &mut epochs,
                        request.allocation_request_id,
                        snapshot.clone(),
                        &[],
                    )
                    .unwrap();
                granted = true;
            }
        }
        std::thread::yield_now();
    }
    peer_thread.join().unwrap();
    assert_eq!(owner.pending(), 0);
    // The final frame's resource stays leased, as in the fresh workload.
    let lease = epochs
        .resources(GRANT)
        .unwrap()
        .lease(GRANT, resource_for_frame(frames))
        .unwrap();
    assert_eq!(lease.cpu_bytes().unwrap().len(), 200 * 20 * 4);
    transport.disconnect(&mut epochs).unwrap();
    drop(transport);
    assert!(!directory.exists());
}
