//! Cross-object coherence and packet-context checks for the image channel
//! (PREFIX role). Mirrors the C `sophia_ic_discovery_coherent`,
//! `sophia_ic_metadata_admissible` and actual-descriptor packet rules with
//! caller-supplied context: no grants are inferred and no state is kept.

use sophia_lock_protocol::lock_files::*;

fn discovery(source_kinds: u32) -> LockImageDiscovery {
    LockImageDiscovery {
        connection_epoch: 11,
        channel_generation: 12,
        endpoint: "image.sock".to_owned(),
        source_kinds,
        max_offers: 3,
        max_packets_per_turn: 2,
        hello_timeout_ms: 300,
        validation_timeout_ms: 1500,
        validation_chunk_bytes: 4096,
        capture_interest_timeout_ms: 250,
        formats_generation: 13,
        limits_generation: 14,
    }
}

fn limits() -> LockImageLimits {
    LockImageLimits {
        channel_generation: 12,
        limits_generation: 14,
        max_resource_bytes: 1 << 16,
        max_sealed_backing_bytes: 1 << 17,
        max_dma_source_bytes: 0,
        max_renderer_allocation_bytes: 0,
        max_renderer_entries: 0,
        max_offers: 3,
        max_validation_jobs: 2,
        max_capture_jobs: 0,
        max_admitted_outputs: 2,
        max_format_rows_per_output: 2,
        max_width: 1024,
        max_height: 512,
        validation_timeout_ms: 1500,
        capture_interest_timeout_ms: 250,
        validation_chunk_bytes: 4096,
        max_packets_per_turn: 2,
    }
}

fn formats(rows: Vec<LockImageFormatRow>) -> LockImageFormats {
    LockImageFormats {
        channel_generation: 12,
        formats_generation: 13,
        limits_generation: 14,
        rows,
    }
}

fn bounds() -> LockImageRoleBounds {
    LockImageRoleBounds {
        max_resource_bytes: 1 << 20,
        max_width: 4096,
        max_height: 4096,
        max_admitted_outputs: 4,
        transfer_slots: 3,
    }
}

#[test]
fn coherent_set_fills_expect_from_limits() {
    let expect = discovery(1)
        .coherent(&limits(), &formats(vec![]), 11, 11, &bounds())
        .unwrap();
    assert_eq!(expect.role, LOCK_IMAGE_CHANNEL_ROLE);
    assert_eq!(expect.connection_epoch, 11);
    assert_eq!(expect.channel_generation, 12);
    assert_eq!(expect.granted_source_kinds, 1);
    assert_eq!(expect.max_width, 1024);
    assert_eq!(expect.max_height, 512);
    assert_eq!(expect.max_resource_bytes, 1 << 16);
}

#[test]
fn coherence_refuses_generation_and_epoch_mismatch() {
    let d = discovery(1);
    assert!(
        d.coherent(&limits(), &formats(vec![]), 99, 11, &bounds())
            .is_err()
    );
    assert!(
        d.coherent(&limits(), &formats(vec![]), 11, 99, &bounds())
            .is_err()
    );
    let mut wrong_gen = limits();
    wrong_gen.limits_generation = 15;
    assert!(
        d.coherent(&wrong_gen, &formats(vec![]), 11, 11, &bounds())
            .is_err()
    );
    let mut wrong_timeouts = limits();
    wrong_timeouts.validation_timeout_ms = 1501;
    assert!(
        d.coherent(&wrong_timeouts, &formats(vec![]), 11, 11, &bounds())
            .is_err()
    );
}

#[test]
fn coherence_refuses_unsupported_formats_and_capacity() {
    // Format rows exist but DMA-BUF is not advertised.
    let row = LockImageFormatRow {
        output: 1,
        device_major: 226,
        device_minor: 0,
        fourcc: 0x3432_5241,
        flags: LOCK_IMAGE_FORMAT_FLAG_OPAQUE,
        modifier: 0,
        max_width: 640,
        max_height: 480,
    };
    assert!(
        discovery(1)
            .coherent(&limits(), &formats(vec![row]), 11, 11, &bounds())
            .is_err()
    );
    // DMA-BUF advertised but the limits grant no capture jobs.
    assert!(
        discovery(2)
            .coherent(&limits(), &formats(vec![]), 11, 11, &bounds())
            .is_err()
    );
    // Role bounds cap what the limits may offer.
    let mut tight = bounds();
    tight.max_width = 512;
    assert!(
        discovery(1)
            .coherent(&limits(), &formats(vec![]), 11, 11, &tight)
            .is_err()
    );
    // Offer ceiling is min(4, transfer slots).
    let mut slots = bounds();
    slots.transfer_slots = 8;
    assert!(
        discovery(1)
            .coherent(&limits(), &formats(vec![]), 11, 11, &slots)
            .is_err()
    );
}

fn sealed_packet() -> (Vec<u8>, LockImageMetadata) {
    let metadata = LockImageMetadata {
        offer_id: 21,
        resource: 22,
        resource_generation: 23,
        admission_epoch: 24,
        output_id: 25,
        allocation_id: 26,
        topology_generation: 27,
        source_kind: LOCK_IMAGE_SOURCE_SEALED_MEMORY,
        source_format: LOCK_IMAGE_SEALED_FORMAT,
        width: 4,
        height: 2,
        stride: 16,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 32,
        modifier: 0,
    };
    let packet = LockImagePacket::Source {
        connection_epoch: 11,
        channel_generation: 12,
        metadata,
        planes: [
            LockImagePlane {
                offset: 0,
                stride: 16,
                fd_index: 0,
            },
            LockImagePlane {
                offset: 0,
                stride: 0,
                fd_index: 0,
            },
            LockImagePlane {
                offset: 0,
                stride: 0,
                fd_index: 0,
            },
            LockImagePlane {
                offset: 0,
                stride: 0,
                fd_index: 0,
            },
        ],
    };
    (packet.encode().unwrap(), metadata)
}

#[test]
fn packet_context_checks_epoch_generation_fds_and_grant() {
    let expect = discovery(1)
        .coherent(&limits(), &formats(vec![]), 11, 11, &bounds())
        .unwrap();
    let (bytes, _) = sealed_packet();
    assert!(LockImagePacket::decode_with_context(&bytes, 1, &expect).is_ok());
    // Actual descriptor count is the caller's fact, not the header's.
    assert!(LockImagePacket::decode_with_context(&bytes, 0, &expect).is_err());
    assert!(LockImagePacket::decode_with_context(&bytes, 2, &expect).is_err());
    // Wrong epoch and generation expectations refuse.
    let mut wrong = expect;
    wrong.connection_epoch = 99;
    assert!(LockImagePacket::decode_with_context(&bytes, 1, &wrong).is_err());
    let mut wrong = expect;
    wrong.channel_generation = 99;
    assert!(LockImagePacket::decode_with_context(&bytes, 1, &wrong).is_err());
    // A grant that names only DMA-BUF refuses the sealed source.
    let mut dma_only = expect;
    dma_only.granted_source_kinds = 2;
    assert!(LockImagePacket::decode_with_context(&bytes, 1, &dma_only).is_err());
    // Hello carries no descriptors.
    let hello = LockImagePacket::Hello {
        connection_epoch: 11,
        channel_generation: 12,
    }
    .encode()
    .unwrap();
    assert!(LockImagePacket::decode_with_context(&hello, 0, &expect).is_ok());
    assert!(LockImagePacket::decode_with_context(&hello, 1, &expect).is_err());
}

#[test]
fn admissible_enforces_granted_kind_and_limits() {
    let expect = discovery(1)
        .coherent(&limits(), &formats(vec![]), 11, 11, &bounds())
        .unwrap();
    let (_, metadata) = sealed_packet();
    assert!(metadata.admissible(&expect).is_ok());
    let mut wide = metadata;
    wide.width = 2048;
    wide.stride = 8192;
    wide.declared_bytes = 2048 * 2 * 4;
    assert!(wide.admissible(&expect).is_err());
    let mut big = metadata;
    big.height = 5000;
    big.declared_bytes = 4 * 5000 * 4;
    big.stride = 16;
    // Width/height are refused before the byte product matters.
    assert!(big.admissible(&expect).is_err());
}

#[test]
fn coherence_revalidates_directly_constructed_inputs() {
    // A partial DMA-BUF capacity group built by hand, never decoded.
    let mut bad_limits = limits();
    bad_limits.max_dma_source_bytes = 8;
    assert!(
        discovery(1)
            .coherent(&bad_limits, &formats(vec![]), 11, 11, &bounds())
            .is_err()
    );
    // Unsorted rows and a zero output, both constructed directly.
    let row = |output: u64| LockImageFormatRow {
        output,
        device_major: 226,
        device_minor: 0,
        fourcc: 0x3432_5241,
        flags: LOCK_IMAGE_FORMAT_FLAG_OPAQUE,
        modifier: 0,
        max_width: 640,
        max_height: 480,
    };
    let mut dma_limits = limits();
    dma_limits.max_dma_source_bytes = 1 << 16;
    dma_limits.max_renderer_allocation_bytes = 1 << 16;
    dma_limits.max_renderer_entries = 2;
    dma_limits.max_capture_jobs = 1;
    assert!(
        discovery(2)
            .coherent(
                &dma_limits,
                &formats(vec![row(2), row(1)]),
                11,
                11,
                &bounds()
            )
            .is_err()
    );
    assert!(
        discovery(2)
            .coherent(&dma_limits, &formats(vec![row(0)]), 11, 11, &bounds())
            .is_err()
    );
}
