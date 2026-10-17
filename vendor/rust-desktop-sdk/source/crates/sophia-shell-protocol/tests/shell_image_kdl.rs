//! Image body offsets and constraints, bound to the authoritative shell
//! KDL (`spec/sophia-shell-files-v1.kdl`). The existing KDL suite covers
//! image kinds and structure; this file pins the seven image body encoders
//! and the format row to declared offsets with independent fixture values.
//! Byte shape only: no admission, negotiation or coherence is claimed.

#[path = "support/shell_files_kdl/mod.rs"]
#[allow(dead_code)] // This suite uses only the shared image field checks.
mod support;

use sophia_shell_protocol::shell_files::*;
use std::collections::BTreeMap;
use support::checks::{verify_block, verify_rows};
use support::kdl_model::{find_block, parse_kdl};

fn expected(pairs: &[(&'static str, i128)]) -> BTreeMap<&'static str, i128> {
    pairs.iter().copied().collect()
}

fn metadata_values(m: &ShellImageMetadata) -> BTreeMap<&'static str, i128> {
    expected(&[
        ("offer_id", m.offer_id as i128),
        ("resource_id", m.resource as i128),
        ("resource_generation", m.resource_generation as i128),
        ("admission_epoch", m.admission_epoch as i128),
        ("output_id", m.output_id as i128),
        ("allocation_id", m.allocation_id as i128),
        ("topology_generation", m.topology_generation as i128),
        ("source_kind", i128::from(m.source_kind)),
        ("source_format", i128::from(m.source_format)),
        ("width", i128::from(m.width)),
        ("height", i128::from(m.height)),
        ("stride", i128::from(m.stride)),
        ("plane_count", i128::from(m.plane_count)),
        ("descriptor_count", i128::from(m.descriptor_count)),
        ("declared_bytes", m.declared_bytes as i128),
        ("modifier", m.modifier as i128),
    ])
}

#[test]
fn image_metadata_bodies_match_kdl_offsets() {
    let kdl = parse_kdl();
    let sealed = ShellImageMetadata {
        offer_id: 41,
        resource: 42,
        resource_generation: 43,
        admission_epoch: 44,
        output_id: 45,
        allocation_id: 46,
        topology_generation: 47,
        source_kind: SHELL_IMAGE_SOURCE_SEALED_MEMORY,
        source_format: SHELL_IMAGE_SEALED_FORMAT,
        width: 3,
        height: 2,
        stride: 12,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 24,
        modifier: 0,
    };
    let begin = sealed.encode().unwrap();
    let block = find_block(&kdl.bodies, "ImageBegin");
    assert_eq!(block.size, begin.len());
    verify_block(
        &begin,
        0,
        "ImageBegin",
        &block.fields,
        &metadata_values(&sealed),
    );

    let dma = ShellImageMetadata {
        offer_id: 51,
        width: 5,
        height: 3,
        stride: 0,
        plane_count: 2,
        descriptor_count: 2,
        declared_bytes: 60,
        modifier: 0x1122,
        source_kind: SHELL_IMAGE_SOURCE_DMA_BUF,
        source_format: 0x3432_5241,
        ..sealed
    };
    let admitted = dma.encode().unwrap();
    let block = find_block(&kdl.bodies, "ImageAdmitted");
    assert_eq!(block.size, admitted.len());
    verify_block(
        &admitted,
        0,
        "ImageAdmitted",
        &block.fields,
        &metadata_values(&dma),
    );
}

#[test]
fn image_event_bodies_match_kdl_offsets() {
    let kdl = parse_kdl();
    let released = ShellSourceReleased {
        offer_id: 61,
        resource: 62,
        resource_generation: 63,
        disposition: ShellSourceDisposition::DrainedAfterSubmission,
    };
    let bytes = released.encode().unwrap();
    let block = find_block(&kdl.bodies, "SourceReleased");
    assert_eq!(block.size, bytes.len());
    verify_block(
        &bytes,
        0,
        "SourceReleased",
        &block.fields,
        &expected(&[
            ("offer_id", 61),
            ("resource_id", 62),
            ("resource_generation", 63),
            ("disposition", 5),
        ]),
    );

    let ready = ShellChannelReady {
        channel_generation: 64,
        granted_source_kinds: 3,
    };
    let bytes = ready.encode().unwrap();
    let block = find_block(&kdl.bodies, "ChannelReady");
    assert_eq!(block.size, bytes.len());
    verify_block(
        &bytes,
        0,
        "ChannelReady",
        &block.fields,
        &expected(&[("channel_generation", 64), ("granted_source_kinds", 3)]),
    );

    let closed = ShellChannelClosed {
        channel_generation: 65,
        reason: ShellChannelClosedReason::GrantRevoked,
    };
    let bytes = closed.encode().unwrap();
    let block = find_block(&kdl.bodies, "ChannelClosed");
    assert_eq!(block.size, bytes.len());
    verify_block(
        &bytes,
        0,
        "ChannelClosed",
        &block.fields,
        &expected(&[("channel_generation", 65), ("reason", 4)]),
    );
}

#[test]
fn image_limits_body_matches_kdl_offsets() {
    let kdl = parse_kdl();
    let limits = ShellImageLimits {
        channel_generation: 71,
        limits_generation: 72,
        max_resource_bytes: 1 << 20,
        max_sealed_backing_bytes: 1 << 21,
        max_dma_source_bytes: 1 << 22,
        max_renderer_allocation_bytes: 1 << 23,
        max_renderer_entries: 9,
        max_offers: 3,
        max_validation_jobs: 2,
        max_capture_jobs: 1,
        max_admitted_outputs: 5,
        max_format_rows_per_output: 17,
        max_width: 3840,
        max_height: 2160,
        validation_timeout_ms: 1500,
        capture_interest_timeout_ms: 250,
        validation_chunk_bytes: 32768,
        max_packets_per_turn: 2,
    };
    let bytes = limits.encode().unwrap();
    let block = find_block(&kdl.bodies, "ImageLimits");
    assert_eq!(block.size, bytes.len());
    verify_block(
        &bytes,
        0,
        "ImageLimits",
        &block.fields,
        &expected(&[
            ("channel_generation", 71),
            ("limits_generation", 72),
            ("max_resource_bytes", 1 << 20),
            ("max_sealed_backing_bytes", 1 << 21),
            ("max_dma_source_bytes", 1 << 22),
            ("max_renderer_allocation_bytes", 1 << 23),
            ("max_renderer_entries", 9),
            ("max_offers", 3),
            ("max_validation_jobs", 2),
            ("max_capture_jobs", 1),
            ("max_admitted_outputs", 5),
            ("max_format_rows_per_output", 17),
            ("max_width", 3840),
            ("max_height", 2160),
            ("validation_timeout_ms", 1500),
            ("capture_interest_timeout_ms", 250),
            ("validation_chunk_bytes", 32768),
            ("max_packets_per_turn", 2),
        ]),
    );
}

#[test]
fn image_formats_prefix_and_rows_match_kdl_offsets() {
    let kdl = parse_kdl();
    let row = |output: u64, modifier: u64| ShellImageFormatRow {
        output,
        device_major: 226,
        device_minor: 1,
        fourcc: 0x3432_5241,
        flags: SHELL_IMAGE_FORMAT_FLAG_OPAQUE,
        modifier,
        max_width: 1920,
        max_height: 1080,
    };
    let formats = ShellImageFormats {
        channel_generation: 81,
        formats_generation: 82,
        limits_generation: 83,
        rows: vec![row(84, 0), row(85, 7)],
    };
    let bytes = formats.encode().unwrap();
    let prefix = find_block(&kdl.prefixes, "ImageFormats");
    verify_block(
        &bytes,
        0,
        "ImageFormats",
        &prefix.fields,
        &expected(&[
            ("channel_generation", 81),
            ("formats_generation", 82),
            ("limits_generation", 83),
            ("row_count", 2),
        ]),
    );
    let row_block = find_block(&kdl.rows, "ImageFormat");
    assert_eq!(bytes.len(), 32 + 2 * row_block.size);
    let row_values = |output: i128, modifier: i128| {
        expected(&[
            ("output_id", output),
            ("device_major", 226),
            ("device_minor", 1),
            ("fourcc", 0x3432_5241),
            ("flags", 1),
            ("modifier", modifier),
            ("max_width", 1920),
            ("max_height", 1080),
        ])
    };
    verify_rows(
        &bytes,
        32,
        row_block.size,
        "ImageFormat",
        &row_block.fields,
        &[row_values(84, 0), row_values(85, 7)],
    );
}

#[test]
fn encoders_refuse_values_outside_kdl_constraints() {
    // source_kind is min=1 max=2 in both metadata bodies.
    let mut metadata = ShellImageMetadata {
        offer_id: 41,
        resource: 42,
        resource_generation: 43,
        admission_epoch: 44,
        output_id: 45,
        allocation_id: 46,
        topology_generation: 47,
        source_kind: 3,
        source_format: 1,
        width: 1,
        height: 1,
        stride: 4,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 4,
        modifier: 0,
    };
    assert!(metadata.encode().is_err());
    metadata.source_kind = SHELL_IMAGE_SOURCE_SEALED_MEMORY;
    assert!(metadata.encode().is_ok());

    // disposition is min=1 max=5; the enum cannot name 6, so decode of a
    // hand-built body is the refusal path.
    let mut released = ShellSourceReleased {
        offer_id: 61,
        resource: 62,
        resource_generation: 63,
        disposition: ShellSourceDisposition::Retained,
    }
    .encode()
    .unwrap();
    released[24] = 6;
    assert!(ShellSourceReleased::decode(&released).is_err());

    // ChannelReady mask is min=1 max=3.
    assert!(
        ShellChannelReady {
            channel_generation: 64,
            granted_source_kinds: 4,
        }
        .encode()
        .is_err()
    );

    // ImageLimits max_offers is min=1 max=4, checked intrinsically.
    let limits = ShellImageLimits {
        channel_generation: 71,
        limits_generation: 72,
        max_resource_bytes: 8,
        max_sealed_backing_bytes: 8,
        max_dma_source_bytes: 0,
        max_renderer_allocation_bytes: 0,
        max_renderer_entries: 0,
        max_offers: 5,
        max_validation_jobs: 1,
        max_capture_jobs: 0,
        max_admitted_outputs: 1,
        max_format_rows_per_output: 1,
        max_width: 2,
        max_height: 1,
        validation_timeout_ms: 2000,
        capture_interest_timeout_ms: 500,
        validation_chunk_bytes: 65536,
        max_packets_per_turn: 1,
    };
    assert!(limits.encode().is_err());
}
