//! A `ResourceBegin` codec checks structural bounds; the exact chunk count is the
//! negotiated grant's, checked by `ContentResourceBegin::layout` where that
//! grant is known. A codec that checked the prototype's layout refused a
//! valid begin under a reduced `max_chunk_bytes`.
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

/// A valid reduced profile: 32 KiB chunks (frame cap chunk + 48).
fn reduced() -> ContentLimits {
    let mut limits = ContentLimits::prototype(grant());
    limits.max_chunk_bytes = 32768;
    limits.max_frame_payload = 32816;
    limits
}

/// 4096 x 8: 16 KiB rows, so two rows per negotiated chunk (4 chunks) but
/// three per prototype chunk (3 chunks).
fn begin(chunks: u32) -> ContentResourceBegin {
    ContentResourceBegin {
        grant: grant(),
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        width_px: 4096,
        height_px: 8,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: chunks,
        total_bytes: 4096 * 8 * 4,
    }
}

fn file_header() -> ShellFileHeader {
    ShellFileHeader {
        kind: ShellFileKind::ResourceBegin,
        connection_epoch: 11,
        submission_id: 1,
        sequence: 0,
    }
}

fn file_record(value: ContentResourceBegin) -> ShellFileResourceBegin {
    ShellFileResourceBegin {
        transaction: TransactionId::from_raw(7),
        slot: 0,
        record: ShellContentRecord::ResourceBegin(value),
    }
}

#[test]
fn the_negotiated_layout_differs_from_the_prototype_for_this_raster() {
    assert_eq!(reduced().validate(), Ok(()));
    let negotiated = begin(4).layout(&reduced()).unwrap();
    assert_eq!((negotiated.rows_per_chunk, negotiated.chunk_count), (2, 4));
    let prototype = begin(3).layout(&ContentLimits::prototype(grant())).unwrap();
    assert_eq!((prototype.rows_per_chunk, prototype.chunk_count), (3, 3));
    // Each grant refuses the other's count.
    assert!(begin(3).layout(&reduced()).is_err());
    assert!(begin(4).layout(&ContentLimits::prototype(grant())).is_err());
}

#[test]
fn a_begin_under_a_reduced_chunk_encodes_and_decodes_as_a_value() {
    let record = ShellContentRecord::ResourceBegin(begin(4));
    assert_eq!(validate_shell_content_record(&record), Ok(()));
    let bytes = encode_shell_content_value(&record).unwrap();
    assert_eq!(
        decode_shell_content_value(ShellContentValueKind::ResourceBegin, &bytes).unwrap(),
        record
    );
}

#[test]
fn a_begin_under_a_reduced_chunk_crosses_the_file_record_both_ways() {
    let value = file_record(begin(4));
    let bytes = encode_shell_file_resource_begin(file_header(), &value).unwrap();
    assert_eq!(decode_shell_file_resource_begin(&bytes).unwrap(), value);
    // The prototype count is also a well-formed record; which one is right is
    // the negotiated grant's decision (above), not the codec's.
    let prototype = file_record(begin(3));
    let bytes = encode_shell_file_resource_begin(file_header(), &prototype).unwrap();
    assert_eq!(decode_shell_file_resource_begin(&bytes).unwrap(), prototype);
}

/// Every description rule that holds under any valid Limits stays in the
/// record validator: identity, dimensions within the contract ceilings,
/// format, canonical scale, exact byte total within the resource ceiling,
/// and a chunk count no valid grant could lay out.
#[test]
fn malformed_descriptions_are_still_refused_by_the_record_codec() {
    let refused = |mutate: &dyn Fn(&mut ContentResourceBegin)| {
        let mut value = begin(4);
        mutate(&mut value);
        let record = ShellContentRecord::ResourceBegin(value.clone());
        let codec = encode_shell_content_value(&record).is_err();
        let validator = validate_shell_content_record(&record).is_err();
        let file = encode_shell_file_resource_begin(file_header(), &file_record(value)).is_err();
        assert_eq!(
            (codec, validator),
            (file, file),
            "the three entry points agree"
        );
        file
    };
    type Mutation = dyn Fn(&mut ContentResourceBegin);
    let cases: [(&str, &Mutation); 18] = [
        ("zero chunks", &|v| v.chunk_count = 0),
        ("more chunks than rows", &|v| v.chunk_count = 9),
        // At the largest chunk any grant allows (65488), 3 rows fit a chunk,
        // so no grant lays 8 rows out in fewer than 3 chunks.
        ("fewer chunks than any grant allows", &|v| v.chunk_count = 2),
        ("zero width", &|v| {
            v.width_px = 0;
            v.total_bytes = 0;
        }),
        ("width over the ceiling", &|v| {
            v.width_px = 8193;
            v.total_bytes = 8193 * 8 * 4;
        }),
        ("zero height", &|v| {
            v.height_px = 0;
            v.total_bytes = 0;
        }),
        ("height over the ceiling", &|v| {
            v.width_px = 1;
            v.height_px = 4097;
            v.total_bytes = 4097 * 4;
            v.chunk_count = 1;
        }),
        ("bytes over the resource ceiling", &|v| {
            v.width_px = 8192;
            v.height_px = 129;
            v.total_bytes = 8192 * 129 * 4;
            v.chunk_count = 129;
        }),
        ("byte total one row short", &|v| v.total_bytes -= 16384),
        ("byte total four bytes over", &|v| v.total_bytes += 4),
        ("unadmitted pixel format", &|v| v.pixel_format = 2),
        ("non-canonical scale", &|v| {
            v.rendered_scale_numerator = 2;
            v.rendered_scale_denominator = 2;
        }),
        ("zero scale denominator", &|v| {
            v.rendered_scale_denominator = 0
        }),
        ("zero scale numerator", &|v| v.rendered_scale_numerator = 0),
        ("zero resource id", &|v| v.resource.id = 0),
        ("zero resource generation", &|v| v.resource.generation = 0),
        ("zero grant connection epoch", &|v| {
            v.grant.connection_epoch = 0
        }),
        ("zero grant content epoch", &|v| {
            v.grant.content_grant_epoch = 0
        }),
    ];
    for (name, mutate) in cases {
        assert!(refused(mutate), "{name} was accepted");
    }
    // Controls: the boundaries of the chunk-count range are admitted.
    for chunks in [3, 4, 8] {
        assert!(!refused(&|v| v.chunk_count = chunks), "{chunks} chunks");
    }
}

/// A malformed incoming record is refused on decode as well: patch a valid
/// file record's chunk count out of range.
#[test]
fn a_malformed_incoming_begin_is_refused_on_decode() {
    let bytes = encode_shell_file_resource_begin(file_header(), &file_record(begin(4))).unwrap();
    // The value's chunk_count follows grant (16), resource (16), width,
    // height, scale (4 x u32) and pixel_format (u16 + reserved u16).
    let value = SHELL_FILE_HEADER_BYTES + 16;
    let at = value + 16 + 16 + 16 + 4;
    assert_eq!(u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()), 4);
    for chunks in [0u32, 2, 9] {
        let mut patched = bytes.clone();
        patched[at..at + 4].copy_from_slice(&chunks.to_le_bytes());
        assert!(
            decode_shell_file_resource_begin(&patched).is_err(),
            "{chunks} chunks decoded"
        );
    }
}
