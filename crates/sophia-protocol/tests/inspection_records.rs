use sophia_protocol::inspection::*;

fn snapshot() -> InspectionSnapshotRecord {
    let geometry = InspectionRect {
        x: -10,
        y: 20,
        width: 800,
        height: 600,
    };
    let id = InspectionSurfaceId {
        index: 0,
        generation: 9,
    };
    InspectionSnapshotRecord {
        schema: INSPECTION_SCHEMA,
        generation: 2,
        sequence: u64::MAX,
        event_offset: 173,
        loss_generation: 0,
        snapshot: InspectionSnapshot {
            session_generation: u64::MAX,
            wm_epoch: 9,
            scene_generation: 12,
            selected_capabilities: u64::MAX,
            wire: InspectionWire::Files,
            state: InspectionState::Ready,
            outputs: vec![InspectionOutput {
                id: 1,
                generation: 2,
                geometry,
                work_area: geometry,
                focus: Some(id),
            }],
            surfaces: vec![InspectionSurface {
                id,
                state_generation: 13,
                output: Some(1),
                geometry,
            }],
        },
    }
}

#[test]
fn inspection_json_is_lossless_decimal_and_allowlisted() {
    let value = snapshot();
    let bytes = encode_inspection_snapshot(&value).unwrap();
    assert_eq!(decode_inspection_snapshot(&bytes).unwrap(), value);
    let text = format_inspection_snapshot(&value).unwrap();
    assert!(text.contains("\"selected_capabilities\":\"18446744073709551615\""));
    assert!(text.contains("\"output\":\"1\""));
    for key in [
        "token",
        "launch_origins",
        "profile",
        "action",
        "pid",
        "title",
        "classification",
    ] {
        assert!(!text.contains(key), "must not disclose {key}");
    }
    assert_eq!(text.as_bytes(), bytes);
}

#[test]
fn unknown_nested_fields_and_duplicate_fields_are_refused() {
    let bytes = encode_inspection_snapshot(&snapshot()).unwrap();
    for pointer in [
        "",
        "/snapshot",
        "/snapshot/outputs/0",
        "/snapshot/surfaces/0",
        "/snapshot/surfaces/0/id",
        "/snapshot/surfaces/0/geometry",
    ] {
        let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        json.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("token".into(), serde_json::json!("SECRET"));
        assert_eq!(
            decode_inspection_snapshot(&serde_json::to_vec(&json).unwrap()),
            Err(InspectionRecordError::Json)
        );
    }
    let text =
        String::from_utf8(bytes)
            .unwrap()
            .replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1);
    assert_eq!(
        decode_inspection_snapshot(text.as_bytes()),
        Err(InspectionRecordError::Json)
    );
}

#[test]
fn numeric_noncanonical_and_overflow_u64s_are_refused() {
    let bytes = encode_inspection_snapshot(&snapshot()).unwrap();
    for invalid in [
        serde_json::json!(9),
        serde_json::json!("09"),
        serde_json::json!("+9"),
        serde_json::json!("-1"),
        serde_json::json!("18446744073709551616"),
        serde_json::json!(null),
    ] {
        let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        json["snapshot"]["wm_epoch"] = invalid;
        assert_eq!(
            decode_inspection_snapshot(&serde_json::to_vec(&json).unwrap()),
            Err(InspectionRecordError::Json)
        );
    }
}

#[test]
fn rows_identity_and_serialized_byte_bounds_are_enforced() {
    let mut record = snapshot();
    record
        .snapshot
        .surfaces
        .push(record.snapshot.surfaces[0].clone());
    assert_eq!(
        encode_inspection_snapshot(&record),
        Err(InspectionRecordError::Identity)
    );
    record = snapshot();
    record.snapshot.surfaces[0].output = Some(2);
    assert_eq!(
        encode_inspection_snapshot(&record),
        Err(InspectionRecordError::Identity)
    );
    let mut json: serde_json::Value =
        serde_json::from_slice(&encode_inspection_snapshot(&snapshot()).unwrap()).unwrap();
    let row = json["snapshot"]["outputs"][0].clone();
    json["snapshot"]["outputs"] = serde_json::Value::Array(vec![row; INSPECTION_MAX_OUTPUTS + 1]);
    assert_eq!(
        decode_inspection_snapshot(&serde_json::to_vec(&json).unwrap()),
        Err(InspectionRecordError::Json)
    );
    assert_eq!(
        decode_inspection_snapshot(&vec![b' '; INSPECTION_MAX_SNAPSHOT_BYTES + 1]),
        Err(InspectionRecordError::Bounds)
    );
}

#[test]
fn schema_truncation_trailing_bytes_and_unknown_enums_are_refused() {
    let bytes = encode_inspection_snapshot(&snapshot()).unwrap();
    for n in [0, 1, bytes.len() / 2, bytes.len() - 2] {
        assert!(decode_inspection_snapshot(&bytes[..n]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.extend_from_slice(b"{}");
    assert_eq!(
        decode_inspection_snapshot(&trailing),
        Err(InspectionRecordError::Json)
    );
    let mut value = snapshot();
    value.schema = 2;
    assert_eq!(
        encode_inspection_snapshot(&value),
        Err(InspectionRecordError::Schema)
    );
    let text = String::from_utf8(bytes)
        .unwrap()
        .replace("\"ready\"", "\"physical_complete\"");
    assert!(decode_inspection_snapshot(text.as_bytes()).is_err());
}

#[test]
fn status_and_payload_free_events_have_no_authority_fields() {
    let status = InspectionStatus {
        schema: 1,
        session_generation: 0,
        state: InspectionState::Unavailable,
        wire: None,
        selected_capabilities: 0,
        generation: 2,
        sequence: 0,
        wm_epoch: 0,
        event_floor: 0,
        event_tail: 0,
        loss_generation: 1,
        snapshot_available: false,
    };
    assert_eq!(
        decode_inspection_status(&encode_inspection_status(&status).unwrap()).unwrap(),
        status
    );
    for event in [
        InspectionEvent::ConfigurationRejected,
        InspectionEvent::SessionOperationAccepted,
        InspectionEvent::SessionOperationRejected,
    ] {
        let value = InspectionEventRecord {
            schema: 1,
            generation: 2,
            sequence: 3,
            loss_generation: 0,
            event,
        };
        let bytes = encode_inspection_event(&value).unwrap();
        assert_eq!(decode_inspection_event(&bytes).unwrap(), value);
        let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        json["operation_token"] = serde_json::json!("SECRET");
        assert!(decode_inspection_event(&serde_json::to_vec(&json).unwrap()).is_err());
    }
}

#[test]
fn sanitizer_preserves_unknown_capabilities_and_sorts_only_safe_rows() {
    let mut value = snapshot().snapshot;
    let mut other = value.outputs[0].clone();
    other.id = 3;
    other.focus = None;
    value.outputs.insert(0, other);
    let sanitized = sanitize_inspection_snapshot(value).unwrap();
    assert_eq!(
        sanitized.outputs.iter().map(|o| o.id).collect::<Vec<_>>(),
        [1, 3]
    );
    assert_eq!(sanitized.selected_capabilities, u64::MAX);
}
