use sophia_protocol::*;

fn context(output: u64) -> PolicyOutputLaunchContext {
    PolicyOutputLaunchContext {
        output: OutputId::from_raw(output),
        output_generation: 3,
        epoch: 7,
        token: output + 10,
    }
}

#[test]
fn output_bookmarks_roundtrip_exact_id_generation_epoch_and_token() {
    let records = [context(1), context(2)];
    let chunks = encode_wm_output_launch_contexts(&records, 7, 4).unwrap();
    assert_eq!(chunks[0].data.len(), 64);
    assert_eq!(decode_wm_output_launch_contexts(&chunks).unwrap(), records);
    assert_eq!(chunks[0].ordinal, 4);
}

#[test]
fn output_bookmarks_refuse_ambiguous_zero_stale_and_unbounded_records() {
    for records in [
        vec![context(0)],
        vec![context(1), context(1)],
        vec![context(1); 17],
    ] {
        assert!(encode_wm_output_launch_contexts(&records, 7, 0).is_err());
    }
    for offset in [0, 8, 16, 24] {
        let mut chunks = encode_wm_output_launch_contexts(&[context(1)], 7, 0).unwrap();
        chunks[0].data[offset..offset + 8].fill(0);
        assert!(decode_wm_output_launch_contexts(&chunks).is_err());
    }
    let mut chunks = encode_wm_output_launch_contexts(&[context(1)], 7, 0).unwrap();
    chunks[0].connection_epoch = 8;
    assert!(decode_wm_output_launch_contexts(&chunks).is_err());
    chunks[0].data.pop();
    assert!(decode_wm_output_launch_contexts(&chunks).is_err());
}
