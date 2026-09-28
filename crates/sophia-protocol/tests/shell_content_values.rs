//! The shell content invariants of `shell_content_wire.rs` on the SDK's
//! neutral value codec and validators, without the socket codecs. A socket
//! frame is a 24-byte header around these value bytes, so every frame offset
//! mutation below is the frame offset less 24. The 24-byte header, the frame
//! transaction rule and header length rewrites stay in the socket test and
//! retire with the socket wire (t269).
#[path = "support/shell_content_records.rs"]
mod fixtures;

use sophia_protocol::shell::encoding::ValueError;
use sophia_protocol::shell::encoding::content::{
    ShellContentValueKind, decode_shell_content_value, encode_shell_content_value,
    shell_content_value_kind,
};
use sophia_protocol::*;

fn value(record: &ShellContentRecord) -> Vec<u8> {
    encode_shell_content_value(record).unwrap()
}

fn decode(record: &ShellContentRecord, bytes: &[u8]) -> Result<ShellContentRecord, ValueError> {
    decode_shell_content_value(shell_content_value_kind(record), bytes)
}

/// `every_record_has_the_admitted_wire_size_and_round_trips`.
#[test]
fn every_record_value_has_the_admitted_size_and_round_trips() {
    // Independently summed from Appendix A, including C.
    let sizes = [
        12, 264, 72, 120, 160, 64, 48, 56, 48, 32, 32, 34, 80, 184, 40, 68, 58, 64, 48, 112, 112,
    ];
    let records = fixtures::fixtures();
    assert_eq!(records.len(), sizes.len());
    for (record, size) in records.iter().zip(sizes) {
        let bytes = value(record);
        assert_eq!(bytes.len(), size, "{record:?}");
        assert_eq!(&decode(record, &bytes).unwrap(), record);
        assert_eq!(value(&decode(record, &bytes).unwrap()), bytes);
    }
}

/// `malformed_records_are_rejected_including_exact_format_mask_cases`: the
/// same corpus mutations at value offsets.
#[test]
fn malformed_record_values_are_rejected_including_exact_format_mask_cases() {
    let records = fixtures::fixtures();
    for (i, record) in records.iter().enumerate() {
        let bytes = value(record);
        let mut truncated = bytes.clone();
        truncated.pop();
        assert!(decode(record, &truncated).is_err(), "truncated-{}", i + 160);
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode(record, &trailing).is_err(), "trailing-{}", i + 160);
        if i > 0 {
            let mut epoch = bytes.clone();
            epoch[0..8].fill(0);
            assert!(decode(record, &epoch).is_err(), "zero-epoch-{}", i + 160);
        }
    }
    for mask in [0u64, 2, 3] {
        let mut bytes = value(&records[1]);
        bytes[64..72].copy_from_slice(&mask.to_le_bytes());
        assert!(decode(&records[1], &bytes).is_err(), "mask {mask}");
    }
    let mut format = value(&records[5]);
    format[48..50].copy_from_slice(&2u16.to_le_bytes());
    assert!(decode(&records[5], &format).is_err(), "unadmitted-format");
    let mut count = value(&records[13]);
    count[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        decode(&records[13], &count).is_err(),
        "surface-count-overflow"
    );
    let mut reserved = value(&records[5]);
    reserved[50] = 1;
    assert!(decode(&records[5], &reserved).is_err(), "resource-reserved");
}

/// `row_chunking_matches_the_three_independent_worked_examples`, verbatim.
#[test]
fn row_chunking_matches_the_three_independent_worked_examples() {
    let grant = fixtures::grant();
    let limits = ContentLimits::prototype(grant);
    for (width, height, rows, chunks) in [(2560, 32, 6, 6), (120, 32, 136, 1), (8192, 128, 1, 128)]
    {
        let description = ContentResourceBegin {
            grant,
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
        };
        assert_eq!(description.layout(&limits).unwrap().rows_per_chunk, rows);
        let mut invalid = description.clone();
        invalid.total_bytes += 4;
        assert!(invalid.layout(&limits).is_err());
        invalid = description.clone();
        invalid.chunk_count += 1;
        assert!(invalid.layout(&limits).is_err());
    }
}

/// `joint_limits_reject_large_rectangle_and_noncanonical_scale_before_allocation`,
/// verbatim. Its input-queue relation is stated in socket frame terms and
/// is on the file contract's purge list; it is kept while the rule stands.
#[test]
fn joint_limits_reject_large_rectangle_and_noncanonical_scale_before_allocation() {
    let grant = fixtures::grant();
    let limits = ContentLimits::prototype(grant);
    let mut value = ContentResourceBegin {
        grant,
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        width_px: 8192,
        height_px: 4096,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 4096,
        total_bytes: 8192 * 4096 * 4,
    };
    assert!(value.layout(&limits).is_err());
    value.height_px = 128;
    value.total_bytes = 4194304;
    value.chunk_count = 128;
    assert!(value.layout(&limits).is_ok());
    value.rendered_scale_numerator = 2;
    value.rendered_scale_denominator = 2;
    assert!(value.layout(&limits).is_err());
    let mut small = limits.clone();
    small.max_chunk_bytes = 32767;
    assert!(small.validate().is_err());
    small = limits;
    small.max_input_queue_bytes = 65536;
    assert!(small.validate().is_err());
}

/// `a_prepared_outcome_cannot_claim_native_presentation`.
#[test]
fn a_prepared_outcome_value_cannot_claim_native_presentation() {
    let ShellContentRecord::CandidateOutcome(mut outcome) = fixtures::fixtures().remove(15) else {
        panic!()
    };
    assert!(
        validate_shell_content_record(&ShellContentRecord::CandidateOutcome(outcome.clone()))
            .is_ok()
    );
    outcome.kind = 1;
    let record = ShellContentRecord::CandidateOutcome(outcome);
    assert!(validate_shell_content_record(&record).is_err());
    assert!(encode_shell_content_value(&record).is_err());
}

/// `content_frame_errors_keep_their_precedence`, the value half: structure
/// before semantics. The frame's transaction rule is the socket's own.
#[test]
fn content_value_errors_keep_structure_before_semantics() {
    let decode = |len: usize| {
        decode_shell_content_value(ShellContentValueKind::FrameDemandCancel, &vec![0u8; len])
    };
    assert_eq!(decode(47), Err(ValueError::Truncated));
    assert_eq!(decode(49), Err(ValueError::TrailingBytes(1)));
    assert!(matches!(decode(48), Err(ValueError::InvalidRecord(_))));
}
