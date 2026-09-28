use sophia_protocol::output_files::*;

const LITERAL: [u8; 40] = [
    1, 0, 16, 0, 16, 0, 128, 0, 4, 0, 64, 0, 0, 8, 0, 0, 64, 0, 0, 0, 0, 64, 0, 0, 248, 6, 0, 0,
    224, 46, 0, 0, 208, 7, 0, 0, 0, 16, 0, 0,
];

#[test]
fn literal_limits_bind_journal_staging_deadlines_and_domain_replay_capacity() {
    let limits = decode_output_file_limits(&LITERAL).unwrap();
    assert_eq!(limits, OutputFileLimits::default());
    assert_eq!(limits.max_domain_transactions, 4096);
    assert_eq!(limits.staging_bytes, 1784);
    assert_eq!(limits.assembly_timeout_millis, 12000);
    assert_eq!(limits.ack_progress_timeout_millis, 2000);
    assert_eq!(encode_output_file_limits(limits).unwrap(), LITERAL);
    let reduced = OutputFileLimits {
        journal_records: 8,
        journal_bytes: 2048,
        assembly_timeout_millis: 1,
        ack_progress_timeout_millis: 1,
        max_domain_transactions: 1,
        ..limits
    };
    assert_eq!(
        decode_output_file_limits(&encode_output_file_limits(reduced).unwrap()).unwrap(),
        reduced
    );
}

#[test]
fn every_limit_refuses_values_outside_its_advertised_interval() {
    for (offset, minimum, maximum) in [
        (16, 8u32, 64u32),
        (20, 2048, 16384),
        (24, 1784, 1784),
        (28, 1, 12000),
        (32, 1, 2000),
        (36, 1, 4096),
    ] {
        for value in [0, minimum - 1, maximum + 1, u32::MAX] {
            let mut bytes = LITERAL;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(
                decode_output_file_limits(&bytes).is_err(),
                "offset {offset}, value {value}"
            );
            let mut limits = OutputFileLimits::default();
            match offset {
                16 => limits.journal_records = value,
                20 => limits.journal_bytes = value,
                24 => limits.staging_bytes = value,
                28 => limits.assembly_timeout_millis = value,
                32 => limits.ack_progress_timeout_millis = value,
                _ => limits.max_domain_transactions = value,
            }
            assert!(encode_output_file_limits(limits).is_err());
        }
    }
}

#[test]
fn fixed_caps_reserved_bytes_and_exact_length_are_enforced() {
    for offset in (0..14).step_by(2) {
        let mut bytes = LITERAL;
        bytes[offset] ^= 1;
        assert!(decode_output_file_limits(&bytes).is_err());
    }
    for offset in [14, 15] {
        let mut bytes = LITERAL;
        bytes[offset] = 1;
        assert!(decode_output_file_limits(&bytes).is_err());
    }
    for end in 0..40 {
        assert!(decode_output_file_limits(&LITERAL[..end]).is_err());
    }
    let mut extra = LITERAL.to_vec();
    extra.push(0);
    assert!(decode_output_file_limits(&extra).is_err());
}
