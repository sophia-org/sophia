//! The lock half of the accepted image-channel contract
//! (`docs/sophia-image-channel.md`): the seven image kinds, their bodies,
//! the channel packets and the discovery text. These tests bind the native
//! lock codec to `protocol/golden/sophia-image-channel-v1.tsv`; they prove
//! byte shape only, never admission or descriptor custody.

use sophia_protocol::lock_files::*;

fn hex(bytes: &str) -> Vec<u8> {
    assert!(bytes.len().is_multiple_of(2));
    (0..bytes.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&bytes[at..at + 2], 16).unwrap())
        .collect()
}

fn golden(name: &str) -> Vec<u8> {
    let corpus = std::fs::read_to_string("../../protocol/golden/sophia-image-channel-v1.tsv")
        .expect("image channel corpus");
    for line in corpus.lines().skip(1) {
        let mut fields = line.split('\t');
        if fields.next() == Some(name) {
            let domain = fields.next().unwrap();
            let accepted = fields.next().unwrap();
            let _descriptors = fields.next().unwrap();
            let bytes = fields.next().unwrap();
            assert_eq!(domain, "record", "{name} is not a record case");
            assert_eq!(accepted, "1", "{name} must be the accepted case");
            return hex(bytes);
        }
    }
    panic!("missing golden case {name}");
}

fn record(name: &str, class: LockFileClass, kind: LockFileKind) -> (LockFileHeader, Vec<u8>) {
    let bytes = golden(name);
    let decoded = decode_lock_file_record(&bytes, class).unwrap();
    assert_eq!(decoded.header.kind, kind, "{name}");
    assert_eq!(
        encode_lock_file_record(decoded.header, decoded.body).unwrap(),
        bytes,
        "{name} must re-encode byte for byte"
    );
    (decoded.header, decoded.body.to_vec())
}

#[test]
fn image_kinds_decode_with_explicit_classes() {
    use LockFileKind as K;
    for (value, kind, class) in [
        (28_672, K::ImageBegin, LockFileClass::Candidate),
        (28_673, K::ImageAdmitted, LockFileClass::Event),
        (28_674, K::SourceReleased, LockFileClass::Event),
        (28_675, K::ChannelReady, LockFileClass::Event),
        (28_676, K::ChannelClosed, LockFileClass::Event),
        (28_677, K::ImageLimits, LockFileClass::Object),
        (28_678, K::ImageFormats, LockFileClass::Object),
    ] {
        assert_eq!(LockFileKind::decode(value), Ok(kind));
        assert_eq!(kind.class(), class, "{kind:?}");
    }
    // The ranges either side of the allocation stay unassigned.
    assert!(LockFileKind::decode(28_671).is_err());
    assert!(LockFileKind::decode(28_679).is_err());
}

#[test]
fn golden_image_records_round_trip() {
    let (_, body) = record(
        "image-begin",
        LockFileClass::Candidate,
        LockFileKind::ImageBegin,
    );
    let begin = LockImageMetadata::decode(&body).unwrap();
    assert_eq!(begin.offer_id, 3);
    assert_eq!(begin.source_kind, LOCK_IMAGE_SOURCE_SEALED_MEMORY);
    // An ImageBegin candidate is exactly the candidate record ceiling.
    assert_eq!(golden("image-begin").len(), 128);

    let (_, body) = record(
        "image-admitted",
        LockFileClass::Event,
        LockFileKind::ImageAdmitted,
    );
    assert_eq!(LockImageMetadata::decode(&body).unwrap(), begin);

    let (_, body) = record(
        "source-released",
        LockFileClass::Event,
        LockFileKind::SourceReleased,
    );
    let released = LockSourceReleased::decode(&body).unwrap();
    assert_eq!(released.offer_id, 3);
    assert_eq!(released.disposition, LockSourceDisposition::Retained);
    assert_eq!(released.encode().unwrap(), body);

    let (_, body) = record(
        "channel-ready",
        LockFileClass::Event,
        LockFileKind::ChannelReady,
    );
    let ready = LockChannelReady::decode(&body).unwrap();
    assert_eq!(ready.channel_generation, 2);
    assert_eq!(ready.granted_source_kinds, 1);
    assert_eq!(ready.encode().unwrap(), body);

    let (_, body) = record(
        "channel-closed",
        LockFileClass::Event,
        LockFileKind::ChannelClosed,
    );
    let closed = LockChannelClosed::decode(&body).unwrap();
    assert_eq!(closed.channel_generation, 2);
    assert_eq!(closed.reason, LockChannelClosedReason::PeerDisconnect);
    assert_eq!(closed.encode().unwrap(), body);
}

#[test]
fn golden_lock_objects_round_trip() {
    let (_, body) = record(
        "lock-limits",
        LockFileClass::Object,
        LockFileKind::ImageLimits,
    );
    let limits = LockImageLimits::decode(&body).unwrap();
    assert_eq!(limits.channel_generation, 2);
    assert_eq!(limits.max_offers, 4);
    assert_eq!(limits.encode().unwrap(), body);
    assert_eq!(golden("lock-limits").len(), 136);

    let (_, body) = record(
        "lock-sealed-formats",
        LockFileClass::Object,
        LockFileKind::ImageFormats,
    );
    let formats = LockImageFormats::decode(&body).unwrap();
    assert_eq!(formats.formats_generation, 9);
    assert!(formats.rows.is_empty(), "sealed-only formats carry no rows");
    assert_eq!(formats.encode().unwrap(), body);
}

#[test]
fn golden_lock_packets_and_discovery_round_trip() {
    let corpus = std::fs::read_to_string("../../protocol/golden/sophia-image-channel-v1.tsv")
        .expect("image channel corpus");
    let mut found = 0;
    for line in corpus.lines().skip(1) {
        let mut fields = line.split('\t');
        let (Some(name), Some(domain), Some(accepted), _, Some(bytes)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        match (name, domain) {
            ("lock-hello" | "lock-sealed-source", "channel") => {
                assert_eq!(accepted, "1");
                let packet = LockImagePacket::decode(&hex(bytes)).unwrap();
                assert_eq!(packet.encode().unwrap(), hex(bytes), "{name}");
                found += 1;
            }
            ("lock-discovery", "discovery") => {
                assert_eq!(accepted, "1");
                let text = String::from_utf8(hex(bytes)).unwrap();
                let discovery = LockImageDiscovery::decode(&text).unwrap();
                assert_eq!(discovery.encode(), text);
                found += 1;
            }
            _ => {}
        }
    }
    assert_eq!(found, 3);
}

#[test]
fn metadata_refuses_noncanonical_sources() {
    let sealed = LockImageMetadata {
        offer_id: 1,
        resource: 3,
        resource_generation: 4,
        admission_epoch: 5,
        output_id: 6,
        allocation_id: 7,
        topology_generation: 9,
        source_kind: LOCK_IMAGE_SOURCE_SEALED_MEMORY,
        source_format: LOCK_IMAGE_SEALED_FORMAT,
        width: 2,
        height: 1,
        stride: 8,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 8,
        modifier: 0,
    };
    assert!(sealed.encode().is_ok());
    // A sealed stride that is not width * 4, a sealed modifier, and a
    // DMA-BUF metadata stride are each refused at the codec.
    assert!(
        LockImageMetadata {
            stride: 9,
            ..sealed
        }
        .encode()
        .is_err()
    );
    assert!(
        LockImageMetadata {
            modifier: 1,
            ..sealed
        }
        .encode()
        .is_err()
    );
    assert!(
        LockImageMetadata {
            source_kind: LOCK_IMAGE_SOURCE_DMA_BUF,
            source_format: 0x3432_4241,
            stride: 8,
            ..sealed
        }
        .encode()
        .is_err()
    );
    // Declared bytes must be the checked pixel count.
    assert!(
        LockImageMetadata {
            declared_bytes: 9,
            ..sealed
        }
        .encode()
        .is_err()
    );
}

#[test]
fn formats_refuse_unsorted_or_flagless_rows() {
    let row = LockImageFormatRow {
        output: 1,
        device_major: 226,
        device_minor: 0,
        fourcc: 0x3432_5241,
        flags: LOCK_IMAGE_FORMAT_FLAG_OPAQUE,
        modifier: 0,
        max_width: 1024,
        max_height: 1024,
    };
    let formats = LockImageFormats {
        channel_generation: 2,
        formats_generation: 9,
        limits_generation: 1,
        rows: vec![row],
    };
    assert!(formats.encode().is_ok());
    assert!(
        LockImageFormats {
            rows: vec![row, row],
            ..formats.clone()
        }
        .encode()
        .is_err()
    );
    assert!(
        LockImageFormats {
            rows: vec![LockImageFormatRow { flags: 0, ..row }],
            ..formats
        }
        .encode()
        .is_err()
    );
}

#[test]
fn image_formats_may_pass_the_general_record_bound() {
    let rows = (1..=2000u64)
        .map(|output| LockImageFormatRow {
            output,
            device_major: 226,
            device_minor: 0,
            fourcc: 0x3432_5241,
            flags: LOCK_IMAGE_FORMAT_FLAG_OPAQUE,
            modifier: 0,
            max_width: 1024,
            max_height: 1024,
        })
        .collect();
    let formats = LockImageFormats {
        channel_generation: 2,
        formats_generation: 9,
        limits_generation: 1,
        rows,
    };
    let body = formats.encode().unwrap();
    let bytes = encode_lock_file_record(
        LockFileHeader {
            kind: LockFileKind::ImageFormats,
            connection_epoch: 1,
            submission_id: 0,
            sequence: 0,
        },
        &body,
    )
    .unwrap();
    assert!(bytes.len() > LOCK_FILE_MAX_BYTES);
    let decoded = decode_lock_file_record(&bytes, LockFileClass::Object).unwrap();
    assert_eq!(LockImageFormats::decode(decoded.body).unwrap(), formats);

    // The same size in any other kind still refuses.
    let oversized = encode_lock_file_record(
        LockFileHeader {
            kind: LockFileKind::Lock,
            connection_epoch: 1,
            submission_id: 0,
            sequence: 0,
        },
        &body,
    );
    assert!(oversized.is_err());
}

#[test]
fn r2_capability_bits_are_named_but_not_negotiable_yet() {
    assert_eq!(LOCK_FILE_CAPABILITY_SEALED_MEMORY, 1 << 2);
    assert_eq!(LOCK_FILE_CAPABILITY_DMA_BUF, 1 << 3);
    // r1 negotiation still refuses either image bit as a granted capability.
    let granted = LockNegotiated {
        granted_chords: 0,
        granted_capabilities: LOCK_FILE_CAPABILITY_PRESENT | LOCK_FILE_CAPABILITY_SEALED_MEMORY,
    };
    assert!(granted.encode().is_err());
    assert_eq!(
        LOCK_RESOURCE_REASON_IMAGE_CHANNEL_UNAVAILABLE, 9,
        "the image contract's lock reason"
    );
}

#[test]
fn discovery_refuses_shape_errors() {
    let good = LockImageDiscovery {
        connection_epoch: 1,
        channel_generation: 2,
        source_kinds: 1,
        max_offers: 4,
        formats_generation: 9,
        limits_generation: 1,
    };
    let text = good.encode();
    assert_eq!(LockImageDiscovery::decode(&text), Ok(good));
    // A duplicate key, a leading zero, a wrong endpoint and a missing
    // final line feed are failures, never optional discovery.
    assert!(LockImageDiscovery::decode(&format!("{text}role=2\n")).is_err());
    assert!(LockImageDiscovery::decode(&text.replace("role=2", "role=02")).is_err());
    assert!(LockImageDiscovery::decode(&text.replace("image.sock", "../image.sock")).is_err());
    assert!(LockImageDiscovery::decode(text.trim_end()).is_err());
    // The shell role code does not decode as lock discovery.
    assert!(LockImageDiscovery::decode(&text.replace("role=2", "role=1")).is_err());
}
