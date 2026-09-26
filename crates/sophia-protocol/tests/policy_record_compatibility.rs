use sophia_protocol::*;

// The direct legacy codec bounds individual chunks, not their combined output
// inventory. This characterization is not evidence of live wire admission.
#[test]
fn legacy_snapshot_direct_codec_retains_split_seventeen_output_acceptance() {
    let record = WmV1SnapshotOutputRecord {
        output: 1,
        generation: 1,
        focus_index: 0,
        focus_generation: 0,
        x: 0,
        y: 0,
        width: 1,
        height: 1,
        work_x: 0,
        work_y: 0,
        work_width: 1,
        work_height: 1,
    };
    let chunks = [16, 1]
        .into_iter()
        .enumerate()
        .map(|(ordinal, count)| WmV1SnapshotChunk {
            connection_epoch: 1,
            ordinal: ordinal as u16,
            record_kind: SNAPSHOT_OUTPUT_RECORD_KIND,
            item_count: count,
            data: encode_wm_v1_snapshot_output_records(&vec![record.clone(); count as usize])
                .unwrap(),
        })
        .collect();
    let transfer = WmV1SnapshotTransfer {
        transaction: TransactionId::from_raw(1),
        begin: WmV1SnapshotBegin {
            connection_epoch: 1,
            scene_generation: 1,
            active_output: 1,
            chunk_count: 2,
            output_count: 17,
            surface_count: 0,
            action_count: 0,
            session_operation_count: 0,
        },
        chunks,
        end: WmV1SnapshotEnd {
            connection_epoch: 1,
            scene_generation: 1,
            chunk_count: 2,
        },
    };
    assert_eq!(
        decode_wm_v1_policy_snapshot(&transfer)
            .unwrap()
            .scene
            .outputs
            .len(),
        17
    );
}
