//! The file wire's canonical upload chunk is `max_chunk_bytes` (Sophia
//! d040013bb). The earlier `min(max_frame_payload - 48, max_chunk_bytes)`
//! equals it on every Limits object that validates, because
//! `max_chunk_bytes + 48 <= max_frame_payload` stays mandatory. These tests
//! pin the rule at its boundaries, the equivalence on valid Limits, the
//! unchanged Limits bytes and the mandatory +24/+48 relations. They pass with
//! either expression by design: the change preserves every admitted layout.
use sophia_shell_protocol::shell::encoding::content::{
    ShellContentValueKind, decode_shell_content_value, encode_shell_content_value,
};
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 11,
        content_grant_epoch: 3,
    }
}

/// A valid profile with the given frame cap, chunk and width cap; the input
/// queue keeps its prototype value, which already exceeds every frame + 24.
fn limits(frame: u32, chunk: u32, width: u32) -> ContentLimits {
    let mut limits = ContentLimits::prototype(grant());
    limits.max_frame_payload = frame;
    limits.max_chunk_bytes = chunk;
    limits.max_width_px = width;
    limits
}

fn begin(width: u32, height: u32, chunks: u32) -> ContentResourceBegin {
    ContentResourceBegin {
        grant: grant(),
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        width_px: width,
        height_px: height,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: chunks,
        total_bytes: u64::from(width) * u64::from(height) * 4,
    }
}

/// The chunk counts an uploader must declare, for each width and height,
/// from `max_chunk_bytes` alone; the layout admits exactly that count.
fn expected(chunk: u32, width: u32, height: u32) -> (u32, u32) {
    let rows = chunk / (width * 4);
    (rows, height.div_ceil(rows))
}

#[test]
fn a_fixed_chunk_gives_the_same_layout_under_every_valid_frame_cap() {
    let mut checked = 0;
    // Chunk sizes at the width floor, between, and at the prototype maximum;
    // frame caps at the +48 boundary, one past it, and at the cap.
    for (chunk, width_cap) in [(32768, 8192), (40000, 8192), (65488, 8192), (4000, 1000)] {
        // At the chunk maximum, the +48 boundary is itself the frame cap.
        let mut frames = vec![chunk + 48, chunk + 49, 65536];
        frames.retain(|frame| *frame <= 65536);
        frames.dedup();
        for &width in &[1, 7, 1000.min(width_cap), chunk / 4 / 3, chunk / 4] {
            if width == 0 || width > width_cap {
                continue;
            }
            for height in [1, 2, 31, 128, 1024] {
                let cap = ContentLimits::prototype(grant()).max_resource_bytes;
                if u64::from(width) * u64::from(height) * 4 > cap {
                    continue;
                }
                checked += 1;
                let (rows, chunks) = expected(chunk, width, height);
                let mut layouts = Vec::new();
                for &frame in &frames {
                    let l = limits(frame, chunk, width_cap);
                    assert_eq!(l.validate(), Ok(()), "frame {frame} chunk {chunk}");
                    // The earlier expression, as an oracle: equal on valid Limits.
                    assert_eq!((frame - 48).min(chunk), chunk);
                    let layout = begin(width, height, chunks).layout(&l).unwrap();
                    assert_eq!(layout.rows_per_chunk, rows);
                    assert_eq!(layout.chunk_count, chunks);
                    assert!(begin(width, height, chunks + 1).layout(&l).is_err());
                    if chunks > 1 {
                        assert!(begin(width, height, chunks - 1).layout(&l).is_err());
                    }
                    layouts.push(layout);
                }
                assert!(layouts.windows(2).all(|w| w[0] == w[1]));
            }
        }
    }
    // Control: the grid really covered many layouts under each frame cap.
    assert!(checked >= 60, "only {checked} layouts");
}

/// One row exactly fills a chunk; a chunk one byte smaller than the row
/// cannot carry it, which the width floor makes unreachable in valid Limits.
#[test]
fn a_row_exactly_the_chunk_is_one_row_per_chunk() {
    let l = limits(4048, 4000, 1000);
    assert_eq!(l.validate(), Ok(()));
    let layout = begin(1000, 5, 5).layout(&l).unwrap();
    assert_eq!((layout.row_bytes, layout.rows_per_chunk), (4000, 1));
    let mut narrow = l.clone();
    narrow.max_chunk_bytes = 3999;
    assert!(narrow.validate().is_err(), "chunk below one full row");
}

/// The worked examples, through the real file upload record. The record
/// carries the canonical count for the grant; which count is exact is the
/// negotiated grant's `layout`, and the record codec refuses only counts no
/// grant could lay out (more chunks than rows).
#[test]
fn file_upload_records_defer_the_exact_chunk_count_to_the_granted_layout() {
    let header = ShellFileHeader {
        kind: ShellFileKind::ResourceBegin,
        connection_epoch: 11,
        submission_id: 1,
        sequence: 0,
    };
    let record = |width, height, chunks| ShellFileResourceBegin {
        transaction: TransactionId::from_raw(7),
        slot: 0,
        record: ShellContentRecord::ResourceBegin(begin(width, height, chunks)),
    };
    let prototype = ContentLimits::prototype(grant());
    for (width, height, rows, chunks) in [
        (2560, 32, 6, 6),
        (120, 32, 136, 1),
        (8192, 128, 1, 128),
        (1, 1, 16372, 1),
    ] {
        assert_eq!(
            expected(prototype.max_chunk_bytes, width, height),
            (rows, chunks)
        );
        let bytes =
            encode_shell_file_resource_begin(header, &record(width, height, chunks)).unwrap();
        assert_eq!(
            decode_shell_file_resource_begin(&bytes).unwrap(),
            record(width, height, chunks)
        );
        assert!(begin(width, height, chunks + 1).layout(&prototype).is_err());
        if chunks > 1 {
            assert!(begin(width, height, chunks - 1).layout(&prototype).is_err());
        }
        assert!(
            encode_shell_file_resource_begin(header, &record(width, height, height + 1)).is_err(),
            "more chunks than rows"
        );
    }
}

/// The +48 and +24 relations stay mandatory: one byte short of either is
/// refused, the boundary itself is admitted.
#[test]
fn the_frame_relations_remain_mandatory() {
    let mut l = limits(40048, 40000, 8192);
    assert_eq!(l.validate(), Ok(()));
    l.max_frame_payload = 40047;
    assert!(l.validate().is_err(), "chunk + 48 > frame");
    let mut l = ContentLimits::prototype(grant());
    l.max_input_queue_bytes = l.max_frame_payload + 24;
    assert_eq!(l.validate(), Ok(()));
    l.max_input_queue_bytes -= 1;
    assert!(l.validate().is_err(), "input queue < frame + 24");
    let mut l = ContentLimits::prototype(grant());
    l.max_frame_payload += 1;
    assert!(l.validate().is_err(), "frame above the prototype cap");
}

fn decode_hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The Limits bytes are unchanged: the golden Limits value (the socket
/// corpus frame less its 24-byte header) decodes and re-encodes identically,
/// as the file `limits` object body, with both socket fields intact.
#[test]
fn limits_bytes_are_unchanged() {
    let corpus = include_str!("../../../spec/golden/sophia-shell-content.frames");
    let frame = corpus
        .lines()
        .find_map(|line| line.strip_prefix("content-161 "))
        .map(decode_hex)
        .unwrap();
    let value = &frame[24..];
    let decoded = decode_shell_content_value(ShellContentValueKind::Limits, value).unwrap();
    let ShellContentRecord::Limits(limits) = &decoded else {
        panic!("limits");
    };
    assert_eq!(limits.max_frame_payload, 65536);
    assert_eq!(limits.max_input_queue_bytes, 131072);
    assert_eq!(limits.max_chunk_bytes, 65488);
    assert_eq!(encode_shell_content_value(&decoded).unwrap(), value);
    let header = ShellFileHeader {
        kind: ShellFileKind::Limits,
        connection_epoch: limits.grant.connection_epoch,
        submission_id: 0,
        sequence: 0,
    };
    let object = encode_shell_file_limits(header, limits.clone()).unwrap();
    assert_eq!(&object[SHELL_FILE_HEADER_BYTES..], value);
}
