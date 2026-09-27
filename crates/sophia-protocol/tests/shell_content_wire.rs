use sophia_protocol::*;
#[path = "../examples/support/shell_content_fixtures.rs"]
mod fixtures;

#[test]
fn every_record_has_the_admitted_wire_size_and_round_trips() {
    // These byte counts are independently summed from Appendix A, including C.
    let sizes = [
        12, 264, 72, 120, 160, 64, 48, 56, 48, 32, 32, 34, 80, 184, 40, 68, 58, 64, 48, 112, 112,
    ];
    for ((record, frame), size) in fixtures::fixtures()
        .iter()
        .zip(fixtures::frames())
        .zip(sizes)
    {
        assert_eq!(frame.len(), size + 24, "{record:?}");
        let (tx, decoded) = decode_shell_content_frame(&frame).unwrap();
        assert_eq!(&decoded, record);
        assert_eq!(encode_shell_content_frame(tx, &decoded).unwrap(), frame);
    }
}

#[test]
fn malformed_records_are_rejected_including_exact_format_mask_cases() {
    for (name, bytes) in fixtures::malformed() {
        assert!(
            decode_shell_content_frame(&bytes).is_err(),
            "accepted {name}"
        );
    }
}

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

#[test]
fn a_prepared_outcome_cannot_claim_native_presentation() {
    let ShellContentRecord::CandidateOutcome(mut outcome) = fixtures::fixtures().remove(15) else {
        panic!()
    };
    outcome.kind = 1;
    assert!(
        encode_shell_content_frame(
            TransactionId::from_raw(1),
            &ShellContentRecord::CandidateOutcome(outcome)
        )
        .is_err()
    );
}

/// Structure, then the frame's transaction rule, then semantics: a zero
/// transaction on a record that needs one is reported as such even when the
/// payload is also semantically invalid; truncation and trailing bytes still
/// come first.
#[test]
fn content_frame_errors_keep_their_precedence() {
    let decode = |len: usize| {
        let frame = encode_frame(
            IpcMessageKind::ShellContentFrameDemandCancel,
            TransactionId::from_raw(0),
            &vec![0u8; len],
        )
        .unwrap();
        decode_shell_content_frame(&frame).unwrap_err()
    };
    assert_eq!(decode(48), IpcCodecError::InvalidTransaction(0));
    assert_eq!(decode(47), IpcCodecError::Truncated);
    assert_eq!(decode(49), IpcCodecError::TrailingBytes(1));
    let frame = encode_frame(
        IpcMessageKind::ShellContentFrameDemandCancel,
        TransactionId::from_raw(7),
        &[0u8; 48],
    )
    .unwrap();
    assert!(matches!(
        decode_shell_content_frame(&frame).unwrap_err(),
        IpcCodecError::InvalidRecord(_)
    ));
}
