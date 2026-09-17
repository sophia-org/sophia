use sophia_protocol::*;
#[path = "../examples/support/native_launcher_fixtures.rs"]
mod fixtures;

fn frame(record: &ShellNativeLauncherRecord) -> Vec<u8> {
    encode_shell_native_launcher_frame(TransactionId::from_raw(25), record).unwrap()
}
fn decode(bytes: &[u8]) -> Result<(TransactionId, ShellNativeLauncherRecord), IpcCodecError> {
    decode_shell_native_launcher_frame(bytes)
}
fn changed_payload(original: &[u8], offset: usize, replacement: &[u8]) -> Vec<u8> {
    let mut b = original.to_vec();
    b[24 + offset..24 + offset + replacement.len()].copy_from_slice(replacement);
    b
}

#[test]
fn every_kind_has_exact_length_and_rejects_truncation_and_trailing_bytes() {
    let lengths = [56, 84, 110, 184, 104, 108, 144, 124, 124, 128, 28];
    for (index, record) in fixtures::fixtures().iter().enumerate() {
        let bytes = frame(record);
        let (header, payload) = decode_frame(&bytes).unwrap();
        assert_eq!(header.message_kind as u16, 187 + index as u16);
        assert_eq!(payload.len(), lengths[index], "kind {}", 187 + index);
        assert_eq!(
            decode(&bytes).unwrap(),
            (header.transaction, record.clone())
        );
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err());
        }
        let mut trailing = payload.to_vec();
        trailing.push(0);
        let extended = encode_frame(header.message_kind, header.transaction, &trailing).unwrap();
        assert!(decode(&extended).is_err());
        let mut zero_tx = bytes.clone();
        zero_tx[8..16].fill(0);
        assert!(decode(&zero_tx).is_err());
        assert!(encode_shell_native_launcher_frame(TransactionId::from_raw(0), record).is_err());
    }
}

#[test]
fn every_presented_identity_component_is_required() {
    let focus = frame(&ShellNativeLauncherRecord::Focus(fixtures::binding()));
    for offset in (0..104).step_by(8) {
        assert!(
            decode(&changed_payload(&focus, offset, &0u64.to_le_bytes())).is_err(),
            "{offset}"
        );
    }
    for record in fixtures::fixtures() {
        let bytes = frame(&record);
        assert!(decode(&changed_payload(&bytes, 0, &0u64.to_le_bytes())).is_err());
        assert!(decode(&changed_payload(&bytes, 8, &0u64.to_le_bytes())).is_err());
    }
}

#[test]
fn catalog_rows_selection_and_declared_targets_agree() {
    let ShellNativeLauncherRecord::CandidateBegin(mut v) = fixtures::fixtures()[2].clone() else {
        unreachable!()
    };
    let encode = |v| {
        encode_shell_native_launcher_frame(
            TransactionId::from_raw(1),
            &ShellNativeLauncherRecord::CandidateBegin(v),
        )
    };
    v.selected = 3;
    assert!(encode(v.clone()).is_err());
    v.selected = 2;
    v.rows = vec![2, 2];
    v.content.target_count = 2;
    assert!(encode(v.clone()).is_err());
    v.rows = vec![2, 4097];
    assert!(encode(v.clone()).is_err());
    v.rows = vec![2];
    assert!(encode(v.clone()).is_err());
    v.content.target_count = 1;
    v.content.surface_count = 2;
    assert!(encode(v.clone()).is_err());
    v.content.surface_count = 1;
    v.rows.clear();
    v.content.target_count = 0;
    assert!(encode(v.clone()).is_err());
    v.selected = 0;
    assert!(encode(v).is_ok());
    let original = frame(&fixtures::fixtures()[2]);
    assert!(decode(&changed_payload(&original, 106, &u16::MAX.to_le_bytes())).is_err());
}

#[test]
fn native_chunk_does_not_widen_legacy_roles_or_action_kinds() {
    let ShellNativeLauncherRecord::CandidateChunk(mut chunk) = fixtures::fixtures()[3].clone()
    else {
        unreachable!()
    };
    assert!(
        encode_shell_content_frame(
            TransactionId::from_raw(1),
            &ShellContentRecord::CandidateChunk(chunk.clone())
        )
        .is_err()
    );
    let mut bytes = frame(&ShellNativeLauncherRecord::CandidateChunk(chunk.clone()));
    bytes[6..8].copy_from_slice(&173u16.to_le_bytes());
    assert!(decode_shell_content_frame(&bytes).is_err());
    let encode = |v| {
        encode_shell_native_launcher_frame(
            TransactionId::from_raw(1),
            &ShellNativeLauncherRecord::CandidateChunk(v),
        )
    };
    chunk.surfaces[0].reservation_extent = 1;
    assert!(encode(chunk.clone()).is_err());
    chunk.surfaces[0].reservation_extent = 0;
    chunk.surfaces[0].parent_surface_index = 0;
    assert!(encode(chunk.clone()).is_err());
    chunk.surfaces[0].parent_surface_index = u16::MAX;
    chunk.surfaces[0].role = 1;
    assert!(encode(chunk.clone()).is_err());
    chunk.surfaces[0].role = 3;
    chunk.targets[0].action_kind = 1;
    assert!(encode(chunk.clone()).is_err());
    chunk.targets[0].action_kind = 2;
    chunk.targets[0].surface_index = 1;
    assert!(encode(chunk.clone()).is_err());
    chunk.targets[0].surface_index = 0;
    chunk.targets[0].action_id = 4097;
    assert!(encode(chunk).is_err());
}

#[test]
fn text_is_bounded_utf8_and_accept_requires_the_presented_revision() {
    let ShellNativeLauncherRecord::Input(mut v) = fixtures::fixtures()[6].clone() else {
        unreachable!()
    };
    let encode = |v| {
        encode_shell_native_launcher_frame(
            TransactionId::from_raw(1),
            &ShellNativeLauncherRecord::Input(v),
        )
    };
    for text in [
        "".to_owned(),
        "x".repeat(257),
        "\0".to_owned(),
        "a\nb".to_owned(),
        "a\u{202e}b".to_owned(),
    ] {
        v.text = text;
        assert!(encode(v.clone()).is_err());
    }
    v.text = "é".repeat(128);
    assert!(encode(v.clone()).is_ok());
    v.kind = NativeLauncherInputKind::Accept;
    assert!(encode(v.clone()).is_err());
    v.text.clear();
    assert!(encode(v.clone()).is_err());
    v.event.state_revision = v.event.binding.state_revision;
    assert!(encode(v.clone()).is_ok());
    v.kind = NativeLauncherInputKind::Next;
    assert!(encode(v).is_err());
    let bytes = frame(&fixtures::fixtures()[6]);
    assert!(decode(&changed_payload(&bytes, 132, &[0xff])).is_err());
    assert!(decode(&changed_payload(&bytes, 128, &18u16.to_le_bytes())).is_err());
    assert!(decode(&changed_payload(&bytes, 130, &257u16.to_le_bytes())).is_err());
}

#[test]
fn activation_cause_and_outcome_cannot_erase_the_exact_binding() {
    let ShellNativeLauncherRecord::ActivationOutcome(mut v) = fixtures::fixtures()[9].clone()
    else {
        unreachable!()
    };
    let encode = |v| {
        encode_shell_native_launcher_frame(
            TransactionId::from_raw(1),
            &ShellNativeLauncherRecord::ActivationOutcome(v),
        )
    };
    v.activation.cause = 0;
    assert!(encode(v).is_err());
    v.activation.cause = 2;
    assert!(encode(v).is_ok());
    v.activation.event.state_revision += 1;
    assert!(encode(v).is_err());
    v.activation.event.state_revision -= 1;
    v.status = 2;
    assert!(encode(v).is_err());
    v.reason = ContentReason::Stale as u16;
    assert!(encode(v).is_ok());
    v.status = 6;
    assert!(encode(v).is_err());
}

#[test]
fn reserved_tails_and_release_shape_are_strict() {
    for index in [5, 7, 10] {
        let mut b = frame(&fixtures::fixtures()[index]);
        *b.last_mut().unwrap() = 1;
        assert!(decode(&b).is_err());
    }
    let ShellNativeLauncherRecord::AllocationRequest(mut v) = fixtures::fixtures()[1].clone()
    else {
        unreachable!()
    };
    let encode = |v| {
        encode_shell_native_launcher_frame(
            TransactionId::from_raw(1),
            &ShellNativeLauncherRecord::AllocationRequest(v),
        )
    };
    v.operation = 3;
    assert!(encode(v).is_err());
    v.prior = fixtures::binding().allocation;
    assert!(encode(v).is_err());
    v.desired_width = 0;
    v.desired_height = 0;
    assert!(encode(v).is_ok());
    v.margins.top = 1;
    assert!(encode(v).is_err());
}
