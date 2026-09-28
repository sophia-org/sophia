//! The WM output launch-context records of `output_launch_context.rs` without
//! the socket codecs. Legacy chunk ordinal and chunk epoch checks retired
//! under t269 (`output_launch_context.rs` at 2eeb074e8). This is
//! the WM projection's output bookmark, not the output role.
use sophia_protocol::*;

fn context(output: u64) -> PolicyOutputLaunchContext {
    PolicyOutputLaunchContext {
        output: OutputId::from_raw(output),
        output_generation: 3,
        epoch: 7,
        token: output + 10,
    }
}

fn decode(
    epoch: u64,
    sections: &[PolicyRecordSection],
) -> Result<Vec<PolicyOutputLaunchContext>, BinaryCodecError> {
    let refs = sections
        .iter()
        .map(PolicyRecordSection::as_ref)
        .collect::<Vec<_>>();
    decode_policy_output_launch_contexts_records(epoch, &refs)
}

/// `output_bookmarks_roundtrip_exact_id_generation_epoch_and_token`.
#[test]
fn output_bookmark_records_roundtrip_exact_id_generation_epoch_and_token() {
    let records = [context(1), context(2)];
    let sections = encode_policy_output_launch_contexts_records(&records, 7).unwrap();
    assert_eq!(
        sections[0].kind,
        PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND
    );
    assert_eq!(sections[0].bytes.len(), 64);
    assert_eq!(decode(7, &sections).unwrap(), records);
}

/// `output_bookmarks_refuse_ambiguous_zero_stale_and_unbounded_records`; the
/// chunk's connection epoch becomes the decoder's expected epoch.
#[test]
fn output_bookmark_records_refuse_ambiguous_zero_stale_and_unbounded_records() {
    for records in [
        vec![context(0)],
        vec![context(1), context(1)],
        vec![context(1); 17],
    ] {
        assert!(encode_policy_output_launch_contexts_records(&records, 7).is_err());
    }
    for offset in [0, 8, 16, 24] {
        let mut sections = encode_policy_output_launch_contexts_records(&[context(1)], 7).unwrap();
        sections[0].bytes[offset..offset + 8].fill(0);
        assert!(decode(7, &sections).is_err(), "offset {offset}");
    }
    let mut sections = encode_policy_output_launch_contexts_records(&[context(1)], 7).unwrap();
    assert!(decode(8, &sections).is_err());
    sections[0].bytes.pop();
    assert!(decode(7, &sections).is_err());
}
