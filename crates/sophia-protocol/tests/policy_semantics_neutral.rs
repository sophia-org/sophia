//! WM policy semantics through shared record sections, scalar validators and
//! the WM file cycle. These preserve the policy assertions from
//! `policy_semantics.rs` at 2eeb074e8; legacy chunk/count assertions retired
//! with the socket adapters under t269. Each test names its original test.
use sophia_protocol::wm_files::*;
use sophia_protocol::*;

fn refs(sections: &[PolicyRecordSection]) -> Vec<PolicyRecordSectionRef<'_>> {
    sections.iter().map(PolicyRecordSection::as_ref).collect()
}

fn snapshot_meta() -> PolicySnapshotMetadata {
    PolicySnapshotMetadata {
        connection_epoch: 2,
        scene_generation: 7,
        active_output: OutputId::from_raw(1),
    }
}

fn header(epoch: u64) -> WmFileHeader {
    WmFileHeader {
        kind: WmFileKind::Cycle,
        connection_epoch: epoch,
        submission_id: 0,
        sequence: if wm_file_class(WmFileKind::Cycle) == WmFileClass::Event {
            203
        } else {
            0
        },
    }
}

fn cycle_bytes(request: &PolicyProjectionRequest) -> Result<Vec<u8>, WmFilePayloadError> {
    encode_wm_file_cycle(
        header(request.connection_epoch),
        &WmFileCycle {
            snapshot_transaction: TransactionId::from_raw(7),
            request_transaction: TransactionId::from_raw(9),
            request: request.clone(),
        },
        u64::MAX,
    )
}

fn file_round_trip(request: &PolicyProjectionRequest) -> PolicyProjectionRequest {
    decode_wm_file_cycle(&cycle_bytes(request).unwrap(), u64::MAX)
        .unwrap()
        .request
}

fn interaction(
    phase: PolicyInteractionPhase,
    kind: PolicyInteractionKind,
    axis: PolicyInteractionAxis,
    geometry: Rect,
) -> PolicyProjectionRequest {
    PolicyProjectionRequest {
        connection_epoch: 2,
        request_id: 6,
        scene_generation: 7,
        policy_generation: 3,
        affected_outputs: vec![OutputId::from_raw(1)],
        cause: PolicyRequestCause::Interaction {
            phase,
            kind,
            axis,
            target: SurfaceId::new(3, 1),
            geometry,
        },
    }
}

fn close_window() -> Vec<PolicyActionRegistration> {
    vec![PolicyActionRegistration {
        action: WmActionId::from_raw(4),
        name: "close-window".to_owned(),
        session_operation_slot: Some(1),
    }]
}

fn kinds(sections: &[PolicyRecordSection]) -> Vec<u16> {
    sections.iter().map(|section| section.kind).collect()
}

/// `snapshot_focus_without_its_usable_surface_fails_both_codec_directions`.
#[test]
fn snapshot_focus_without_its_usable_surface_fails_both_record_directions() {
    let mut scene = gated_scene();
    scene.session_operations.clear();

    let mut invalid = scene.clone();
    invalid.surfaces.clear();
    assert!(matches!(
        encode_policy_snapshot_records(2, &invalid, &[], &[], &[], 0),
        Err(BinaryCodecError::InvalidEnum {
            field: "snapshot_output_focus",
            ..
        })
    ));

    let mut sections = encode_policy_snapshot_records(2, &scene, &[], &[], &[], 0).unwrap();
    sections.retain(|section| section.kind != SNAPSHOT_SURFACE_RECORD_KIND);
    assert!(matches!(
        decode_policy_snapshot_records(snapshot_meta(), &refs(&sections)),
        Err(BinaryCodecError::InvalidEnum {
            field: "snapshot_output_focus",
            ..
        })
    ));
}

/// `projection_request_rejects_duplicate_or_truncated_output_ids`, the
/// duplicate half; the truncated fixed array is the legacy layout's own.
#[test]
fn projection_request_rejects_duplicate_affected_outputs() {
    let duplicate = PolicyProjectionRequest {
        connection_epoch: 2,
        request_id: 5,
        scene_generation: 7,
        policy_generation: 3,
        affected_outputs: vec![OutputId::from_raw(1), OutputId::from_raw(1)],
        cause: PolicyRequestCause::SceneChanged,
    };
    assert!(validate_policy_projection_request(&duplicate).is_err());
    assert!(cycle_bytes(&duplicate).is_err());
    let single = PolicyProjectionRequest {
        affected_outputs: vec![OutputId::from_raw(1)],
        ..duplicate
    };
    assert_eq!(file_round_trip(&single), single);
}

/// `policy_configuration_rejects_ambiguous_or_invalid_actions`.
#[test]
fn policy_configuration_records_reject_ambiguous_or_invalid_actions() {
    let action = PolicyActionRegistration {
        action: WmActionId::from_raw(7),
        name: "resize-width 0.1".to_owned(),
        session_operation_slot: None,
    };
    let mut configuration = PolicyConfiguration {
        action_lifecycles: Vec::new(),
        connection_epoch: 2,
        generation: 3,
        actions: vec![action.clone(), action],
        chrome: WmChromePolicy::default(),
    };
    assert!(encode_policy_configuration_records(&configuration).is_err());

    configuration.actions = vec![PolicyActionRegistration {
        action: WmActionId::from_raw(8),
        name: " leading-space".to_owned(),
        session_operation_slot: None,
    }];
    assert!(encode_policy_configuration_records(&configuration).is_err());
}

/// `reduced_interaction_preserves_kind_phase_and_geometry` and
/// `revision_three_interaction_vocabulary_has_one_fixed_payload_contract`.
#[test]
fn interaction_vocabulary_round_trips_through_the_file_cycle() {
    let geometry = Rect {
        x: 20,
        y: 30,
        width: 800,
        height: 600,
    };
    let mut requests = vec![interaction(
        PolicyInteractionPhase::Cancel,
        PolicyInteractionKind::Resize,
        PolicyInteractionAxis::None,
        geometry,
    )];
    for kind in [
        PolicyInteractionKind::Move,
        PolicyInteractionKind::Resize,
        PolicyInteractionKind::Drag,
    ] {
        requests.push(interaction(
            PolicyInteractionPhase::Update,
            kind,
            PolicyInteractionAxis::None,
            geometry,
        ));
    }
    for (phase, delta) in [
        (PolicyInteractionPhase::Begin, -120),
        (PolicyInteractionPhase::Update, -60),
        (PolicyInteractionPhase::End, -30),
        (PolicyInteractionPhase::Cancel, 0),
    ] {
        requests.push(interaction(
            phase,
            PolicyInteractionKind::Scroll,
            PolicyInteractionAxis::Vertical,
            Rect {
                x: 0,
                y: delta,
                width: 0,
                height: 0,
            },
        ));
    }
    for request in requests {
        assert_eq!(validate_policy_projection_request(&request), Ok(()));
        assert_eq!(file_round_trip(&request), request);
    }
}

/// `revision_three_interaction_payload_rejects_ambiguous_encodings`, the
/// payload half. Its unknown axis 3 and kind 5 codes are refused by the
/// neutral code tables in `policy_scalars_neutral.rs`.
#[test]
fn ambiguous_interaction_payloads_are_refused_before_any_file() {
    let scroll = |axis, geometry| {
        interaction(
            PolicyInteractionPhase::Update,
            PolicyInteractionKind::Scroll,
            axis,
            geometry,
        )
    };
    for invalid in [
        scroll(
            PolicyInteractionAxis::None,
            Rect {
                x: 0,
                y: -60,
                width: 0,
                height: 0,
            },
        ),
        scroll(
            PolicyInteractionAxis::Vertical,
            Rect {
                x: 0,
                y: -60,
                width: 1,
                height: 0,
            },
        ),
        interaction(
            PolicyInteractionPhase::Update,
            PolicyInteractionKind::Move,
            PolicyInteractionAxis::Horizontal,
            Rect {
                x: 20,
                y: 30,
                width: 800,
                height: 600,
            },
        ),
        interaction(
            PolicyInteractionPhase::End,
            PolicyInteractionKind::Scroll,
            PolicyInteractionAxis::Horizontal,
            Rect::default(),
        ),
    ] {
        assert!(validate_policy_projection_request(&invalid).is_err());
        assert!(cycle_bytes(&invalid).is_err());
    }
    let valid = scroll(
        PolicyInteractionAxis::Vertical,
        Rect {
            x: 0,
            y: -60,
            width: 0,
            height: 0,
        },
    );
    assert_eq!(file_round_trip(&valid), valid);
    assert_eq!(policy_interaction_axis_from_code(3), None);
    assert_eq!(policy_interaction_kind_from_code(5), None);
}

/// `capability_gating_omits_ungated_content_without_perturbing_the_rest`, on
/// record sections: ungated content is omitted, not counted, and enabling a
/// capability leaves every ungated section byte-identical and in order.
#[test]
fn capability_gating_omits_ungated_sections_without_perturbing_the_rest() {
    let scene = gated_scene();
    let actions = close_window();
    let ungated = encode_policy_snapshot_records(2, &scene, &actions, &[], &[], 0).unwrap();
    let gated = encode_policy_snapshot_records(
        2,
        &scene,
        &actions,
        &[],
        &[],
        SOPHIA_WM_CAPABILITY_ACTIONS | SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
    )
    .unwrap();
    assert_eq!(
        kinds(&ungated),
        [SNAPSHOT_OUTPUT_RECORD_KIND, SNAPSHOT_SURFACE_RECORD_KIND]
    );
    assert_eq!(
        kinds(&gated),
        [
            SNAPSHOT_OUTPUT_RECORD_KIND,
            SNAPSHOT_SURFACE_RECORD_KIND,
            SNAPSHOT_ACTION_RECORD_KIND,
            SNAPSHOT_SESSION_OPERATION_RECORD_KIND,
        ]
    );
    assert_eq!(gated[2].count, 1);
    assert_eq!(gated[3].count, 1);
    assert_eq!(&gated[..2], ungated.as_slice());

    let decoded_ungated = decode_policy_snapshot_records(snapshot_meta(), &refs(&ungated)).unwrap();
    assert!(decoded_ungated.actions.is_empty());
    assert!(decoded_ungated.scene.session_operations.is_empty());
    assert_eq!(decoded_ungated.scene.outputs, scene.outputs);
    assert_eq!(decoded_ungated.scene.surfaces, scene.surfaces);
    let decoded_gated = decode_policy_snapshot_records(snapshot_meta(), &refs(&gated)).unwrap();
    assert_eq!(decoded_gated.scene, scene);
    assert_eq!(decoded_gated.actions, actions);
}

/// `launch_placement_is_an_uncounted_gated_extension`, on record sections.
#[test]
fn launch_placement_is_one_gated_extension_section() {
    let scene = gated_scene();
    let classifications = [PolicySurfaceClassification {
        surface: scene.surfaces[0].surface,
        classification: 2,
    }];
    let baseline = encode_policy_snapshot_records(2, &scene, &[], &[], &[], 0).unwrap();
    let gated_off =
        encode_policy_snapshot_records(2, &scene, &[], &classifications, &[], 0).unwrap();
    assert_eq!(gated_off, baseline);
    let gated = encode_policy_snapshot_records(
        2,
        &scene,
        &[],
        &classifications,
        &[],
        SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT,
    )
    .unwrap();
    assert_eq!(gated.len(), baseline.len() + 1);
    assert_eq!(&gated[..baseline.len()], baseline.as_slice());
    let extension = gated.last().unwrap();
    assert_eq!(extension.kind, SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND);
    assert_eq!(extension.count, 1);
    let decoded = decode_policy_snapshot_records(snapshot_meta(), &refs(&gated)).unwrap();
    assert_eq!(decoded.classifications, classifications);
}

/// `each_governed_capability_gates_only_its_own_record_kind`.
#[test]
fn each_governed_capability_gates_only_its_own_section() {
    let scene = gated_scene();
    let actions = close_window();
    for (capability, kind) in [
        (SOPHIA_WM_CAPABILITY_ACTIONS, SNAPSHOT_ACTION_RECORD_KIND),
        (
            SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
            SNAPSHOT_SESSION_OPERATION_RECORD_KIND,
        ),
    ] {
        let sections =
            encode_policy_snapshot_records(2, &scene, &actions, &[], &[], capability).unwrap();
        assert_eq!(
            kinds(&sections),
            [
                SNAPSHOT_OUTPUT_RECORD_KIND,
                SNAPSHOT_SURFACE_RECORD_KIND,
                kind
            ]
        );
        assert_eq!(sections[2].count, 1);
        decode_policy_snapshot_records(snapshot_meta(), &refs(&sections)).unwrap();
    }
}

/// `launch_origin_extension_roundtrips_without_changing_frozen_counts`.
#[test]
fn launch_origin_extension_is_gated_epoch_checked_and_fails_closed() {
    let scene = gated_scene();
    let context = PolicyLaunchContext {
        surface: scene.surfaces[0].surface,
        epoch: 2,
        token: 41,
    };
    let original = encode_policy_snapshot_records(2, &scene, &[], &[], &[], 0).unwrap();
    assert_eq!(
        encode_policy_snapshot_records(2, &scene, &[], &[], &[context], 0).unwrap(),
        original
    );
    let mut sections = encode_policy_snapshot_records(
        2,
        &scene,
        &[],
        &[],
        &[context],
        SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN,
    )
    .unwrap();
    assert_eq!(&sections[..original.len()], original.as_slice());
    assert_eq!(
        decode_policy_snapshot_records(snapshot_meta(), &refs(&sections))
            .unwrap()
            .launch_origins,
        vec![context]
    );
    let mut stale = context;
    stale.epoch = 1;
    assert!(
        encode_policy_snapshot_records(
            2,
            &scene,
            &[],
            &[],
            &[stale],
            SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN
        )
        .is_err()
    );
    let last = sections.last_mut().unwrap();
    assert_eq!(last.kind, SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND);
    last.bytes[0..4].copy_from_slice(&999_u32.to_le_bytes());
    assert!(decode_policy_snapshot_records(snapshot_meta(), &refs(&sections)).is_err());
}

/// `launch_bookmarks_reject_ambiguous_identity_and_cross_chunk_duplicates`;
/// the cross-chunk half becomes a cross-section duplicate.
#[test]
fn launch_bookmarks_reject_ambiguous_identity_and_cross_section_duplicates() {
    let context = PolicyLaunchContext {
        surface: SurfaceId::new(0, 1),
        epoch: 2,
        token: 41,
    };
    let bytes = encode_wm_launch_context_records(&[context]).unwrap();
    assert_eq!(bytes.len(), 24);
    assert_eq!(
        decode_wm_launch_context_records(&bytes, 1).unwrap(),
        vec![context]
    );
    assert!(decode_wm_launch_context_records(&bytes[..23], 1).is_err());
    for invalid in [
        PolicyLaunchContext {
            surface: SurfaceId::new(u32::MAX, 1),
            ..context
        },
        PolicyLaunchContext {
            surface: SurfaceId::new(0, 0),
            ..context
        },
        PolicyLaunchContext {
            epoch: 0,
            ..context
        },
        PolicyLaunchContext {
            token: 0,
            ..context
        },
    ] {
        assert!(encode_wm_launch_context_records(&[invalid]).is_err());
    }
    let sections = encode_policy_launch_contexts_records(&[context], 2).unwrap();
    assert!(encode_policy_launch_contexts_records(&[context], 3).is_err());
    assert_eq!(
        decode_policy_launch_contexts_records(2, &refs(&sections)).unwrap(),
        vec![context]
    );
    let doubled = [sections.clone(), sections].concat();
    assert!(decode_policy_launch_contexts_records(2, &refs(&doubled)).is_err());
}

/// `pointer_focus.rs`'s zero-index window target, on the file cycle.
#[test]
fn pointer_focus_targets_round_trip_through_the_file_cycle() {
    for target in [None, Some(SurfaceId::new(0, 1)), Some(SurfaceId::new(7, 3))] {
        let request = PolicyProjectionRequest {
            connection_epoch: 2,
            request_id: 6,
            scene_generation: 7,
            policy_generation: 3,
            affected_outputs: vec![OutputId::from_raw(1)],
            cause: PolicyRequestCause::PointerFocus {
                output: OutputId::from_raw(1),
                target,
            },
        };
        assert_eq!(file_round_trip(&request), request);
    }
}

fn gated_scene() -> PolicySceneSnapshot {
    PolicySceneSnapshot {
        generation: 7,
        active_output: OutputId::from_raw(1),
        outputs: vec![PolicyOutputSnapshot {
            policy_key: None,
            output: OutputId::from_raw(1),
            generation: 3,
            focus: Some(SurfaceId::new(3, 1)),
            bounds: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            work_area: Rect {
                x: 0,
                y: 24,
                width: 1920,
                height: 1056,
            },
        }],
        surfaces: vec![surface()],
        session_operations: vec![PolicySessionOperation {
            token: 11,
            slot: 1,
            permits_surface_target: true,
        }],
    }
}

fn surface() -> PolicySurfaceSnapshot {
    PolicySurfaceSnapshot {
        surface: SurfaceId::new(3, 1),
        generation: 8,
        current_output: Some(OutputId::from_raw(1)),
        kind: PolicySurfaceKind::Dialog,
        capabilities: LayoutNodeCapabilities::STANDARD_TOPLEVEL,
        constraints: SurfaceConstraints {
            min_size: Some(Size {
                width: 100,
                height: 80,
            }),
            max_size: None,
        },
        exact_size: None,
        requested_state: PolicyPresentationState {
            fullscreen: true,
            ..PolicyPresentationState::default()
        },
        current_state: PolicyPresentationState::default(),
        transient_owner: None,
        geometry: Rect {
            x: 20,
            y: 30,
            width: 800,
            height: 600,
        },
    }
}
