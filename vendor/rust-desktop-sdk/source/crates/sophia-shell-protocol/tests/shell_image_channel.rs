//! The shell half of the accepted image-channel contract
//! (`docs/sophia-image-channel.md`): image bodies, channel packets and the
//! discovery text for role 1. Record envelopes and kind registration belong
//! to the shell files codec; these tests bind only the new image module to
//! `spec/golden/sophia-image-channel-v1.tsv` and prove byte shape only,
//! never admission, negotiation or descriptor custody.

use sophia_shell_protocol::shell_files::*;

fn hex(bytes: &str) -> Vec<u8> {
    assert!(bytes.len().is_multiple_of(2));
    (0..bytes.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&bytes[at..at + 2], 16).unwrap())
        .collect()
}

fn golden(name: &str) -> (String, String, Vec<u8>) {
    let corpus = std::fs::read_to_string("../../spec/golden/sophia-image-channel-v1.tsv")
        .expect("image channel corpus");
    for line in corpus.lines().skip(1) {
        let mut fields = line.split('\t');
        if fields.next() == Some(name) {
            return (
                fields.next().unwrap().to_owned(),
                fields.next().unwrap().to_owned(),
                hex(fields.nth(1).unwrap()),
            );
        }
    }
    panic!("missing golden case {name}");
}

/// The body of a golden record: the shared 32-byte role header is the
/// envelope owner's, so this test strips it without decoding it.
fn body(name: &str) -> Vec<u8> {
    let (domain, accepted, bytes) = golden(name);
    assert_eq!(domain, "record", "{name}");
    assert_eq!(accepted, "1", "{name}");
    bytes[32..].to_vec()
}

#[test]
fn golden_shell_bodies_round_trip() {
    let begin = ShellImageMetadata::decode(&body("image-begin")).unwrap();
    assert_eq!(begin.offer_id, 3);
    assert_eq!(begin.source_kind, SHELL_IMAGE_SOURCE_SEALED_MEMORY);
    assert_eq!(begin.encode().unwrap()[..], body("image-begin")[..]);
    assert_eq!(
        ShellImageMetadata::decode(&body("image-admitted")).unwrap(),
        begin
    );

    let released = ShellSourceReleased::decode(&body("source-released")).unwrap();
    assert_eq!(released.offer_id, 3);
    assert_eq!(released.disposition, ShellSourceDisposition::Retained);
    assert_eq!(released.encode().unwrap(), body("source-released"));

    let ready = ShellChannelReady::decode(&body("channel-ready")).unwrap();
    assert_eq!(ready.channel_generation, 2);
    assert_eq!(ready.granted_source_kinds, 1);
    assert_eq!(ready.encode().unwrap(), body("channel-ready"));

    let closed = ShellChannelClosed::decode(&body("channel-closed")).unwrap();
    assert_eq!(closed.channel_generation, 2);
    assert_eq!(closed.reason, ShellChannelClosedReason::PeerDisconnect);
    assert_eq!(closed.encode().unwrap(), body("channel-closed"));
}

#[test]
fn golden_shell_objects_round_trip() {
    let limits = ShellImageLimits::decode(&body("shell-limits")).unwrap();
    assert_eq!(limits.channel_generation, 2);
    assert_eq!(limits.max_offers, 4);
    assert_eq!(limits.encode().unwrap(), body("shell-limits"));

    let formats = ShellImageFormats::decode(&body("shell-sealed-formats")).unwrap();
    assert_eq!(formats.formats_generation, 9);
    assert!(formats.rows.is_empty(), "sealed-only formats carry no rows");
    assert_eq!(formats.encode().unwrap(), body("shell-sealed-formats"));
}

#[test]
fn golden_shell_packets_and_discovery_round_trip() {
    for name in ["shell-hello", "shell-sealed-source"] {
        let (domain, accepted, bytes) = golden(name);
        assert_eq!(domain, "channel", "{name}");
        assert_eq!(accepted, "1", "{name}");
        let packet = ShellImagePacket::decode(&bytes).unwrap();
        assert_eq!(packet.encode().unwrap(), bytes, "{name}");
    }
    // The lock packet (role 2) does not decode as a shell packet.
    let (_, _, lock_hello) = golden("lock-hello");
    assert!(ShellImagePacket::decode(&lock_hello).is_err());

    let (domain, accepted, bytes) = golden("shell-discovery");
    assert_eq!(domain, "discovery");
    assert_eq!(accepted, "1");
    let text = String::from_utf8(bytes).unwrap();
    let discovery = ShellImageDiscovery::decode(&text).unwrap();
    assert_eq!(discovery.encode().unwrap(), text);
    assert_eq!(discovery.endpoint, "image.sock");
}

fn sealed_metadata() -> ShellImageMetadata {
    ShellImageMetadata {
        offer_id: 1,
        resource: 3,
        resource_generation: 4,
        admission_epoch: 5,
        output_id: 6,
        allocation_id: 7,
        topology_generation: 9,
        source_kind: SHELL_IMAGE_SOURCE_SEALED_MEMORY,
        source_format: SHELL_IMAGE_SEALED_FORMAT,
        width: 2,
        height: 1,
        stride: 8,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 8,
        modifier: 0,
    }
}

#[test]
fn metadata_refuses_noncanonical_sources() {
    let sealed = sealed_metadata();
    assert!(sealed.encode().is_ok());
    assert!(
        ShellImageMetadata {
            stride: 9,
            ..sealed
        }
        .encode()
        .is_err()
    );
    assert!(
        ShellImageMetadata {
            modifier: 1,
            ..sealed
        }
        .encode()
        .is_err()
    );
    assert!(
        ShellImageMetadata {
            source_kind: SHELL_IMAGE_SOURCE_DMA_BUF,
            source_format: 0x3432_4241,
            stride: 8,
            ..sealed
        }
        .encode()
        .is_err()
    );
    assert!(
        ShellImageMetadata {
            declared_bytes: 9,
            ..sealed
        }
        .encode()
        .is_err()
    );
    // No role maximum in the shared shape: role limits are caller checks.
    assert!(
        ShellImageMetadata {
            width: 20_000,
            height: 1,
            stride: 80_000,
            declared_bytes: 80_000,
            ..sealed
        }
        .encode()
        .is_ok()
    );
}

#[test]
fn limits_groups_are_all_or_nothing() {
    let base = ShellImageLimits {
        channel_generation: 2,
        limits_generation: 1,
        max_resource_bytes: 8,
        max_sealed_backing_bytes: 8,
        max_dma_source_bytes: 0,
        max_renderer_allocation_bytes: 0,
        max_renderer_entries: 0,
        max_offers: 4,
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
    assert!(base.encode().is_ok());
    assert!(
        ShellImageLimits {
            max_sealed_backing_bytes: 0,
            ..base
        }
        .encode()
        .is_err()
    );
    assert!(
        ShellImageLimits {
            max_validation_jobs: 0,
            ..base
        }
        .encode()
        .is_err()
    );
    assert!(
        ShellImageLimits {
            max_sealed_backing_bytes: 7,
            ..base
        }
        .encode()
        .is_err()
    );
    assert!(
        ShellImageLimits {
            max_dma_source_bytes: 8,
            ..base
        }
        .encode()
        .is_err()
    );
    let dma = ShellImageLimits {
        max_dma_source_bytes: 8,
        max_renderer_allocation_bytes: 8,
        max_renderer_entries: 1,
        max_capture_jobs: 2,
        ..base
    };
    assert!(dma.encode().is_ok());
    for partial in [
        ShellImageLimits {
            max_dma_source_bytes: 0,
            ..dma
        },
        ShellImageLimits {
            max_renderer_allocation_bytes: 0,
            ..dma
        },
        ShellImageLimits {
            max_renderer_entries: 0,
            ..dma
        },
        ShellImageLimits {
            max_capture_jobs: 0,
            ..dma
        },
    ] {
        assert!(partial.encode().is_err());
    }
}

#[test]
fn formats_refuse_unsorted_or_flagless_rows() {
    let row = ShellImageFormatRow {
        output: 1,
        device_major: 226,
        device_minor: 0,
        fourcc: 0x3432_5241,
        flags: SHELL_IMAGE_FORMAT_FLAG_OPAQUE,
        modifier: 0,
        max_width: 1024,
        max_height: 1024,
    };
    let formats = ShellImageFormats {
        channel_generation: 2,
        formats_generation: 9,
        limits_generation: 1,
        rows: vec![row],
    };
    assert!(formats.encode().is_ok());
    assert!(
        ShellImageFormats {
            rows: vec![row, row],
            ..formats.clone()
        }
        .encode()
        .is_err()
    );
    assert!(
        ShellImageFormats {
            rows: vec![ShellImageFormatRow { flags: 0, ..row }],
            ..formats
        }
        .encode()
        .is_err()
    );
}

#[test]
fn discovery_refuses_shape_errors() {
    let good = ShellImageDiscovery {
        connection_epoch: 1,
        channel_generation: 2,
        endpoint: "image.sock".to_owned(),
        source_kinds: 1,
        max_offers: 4,
        max_packets_per_turn: 3,
        hello_timeout_ms: 300,
        validation_timeout_ms: 5000,
        validation_chunk_bytes: 4096,
        capture_interest_timeout_ms: 750,
        formats_generation: 9,
        limits_generation: 1,
    };
    let text = good.encode().unwrap();
    assert!(text.contains("role=1\n"));
    assert_eq!(ShellImageDiscovery::decode(&text), Ok(good.clone()));
    assert!(ShellImageDiscovery::decode(&format!("{text}role=1\n")).is_err());
    assert!(ShellImageDiscovery::decode(&text.replace("role=1", "role=01")).is_err());
    // A zero Hello timeout is a positive-field refusal, not a default.
    assert!(
        ShellImageDiscovery::decode(&text.replace("hello_timeout_ms=300", "hello_timeout_ms=0"))
            .is_err()
    );
    assert!(ShellImageDiscovery::decode(&text.replace("image.sock", "../image.sock")).is_err());
    assert!(ShellImageDiscovery::decode(text.trim_end()).is_err());
    // The lock role code does not decode as shell discovery.
    assert!(ShellImageDiscovery::decode(&text.replace("role=1", "role=2")).is_err());
}
