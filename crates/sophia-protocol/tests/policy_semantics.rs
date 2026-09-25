use sophia_protocol::*;

#[test]
fn complete_scene_snapshot_roundtrips_without_policy_private_state() {
    let scene = PolicySceneSnapshot {
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
    };
    let actions = vec![PolicyActionRegistration {
        action: WmActionId::from_raw(4),
        name: "close-window".to_owned(),
        session_operation_slot: Some(1),
    }];
    let transfer = encode_wm_v1_policy_snapshot(
        TransactionId::from_raw(9),
        2,
        &scene,
        &actions,
        &[],
        SOPHIA_WM_CAPABILITY_ACTIONS | SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
    )
    .unwrap();
    let decoded = decode_wm_v1_policy_snapshot(&transfer).unwrap();

    assert_eq!(decoded.scene, scene);
    assert_eq!(decoded.actions, actions);
    assert_eq!(transfer.chunks.len(), 4);
}

#[test]
fn snapshot_focus_without_its_usable_surface_fails_both_codec_directions() {
    let mut scene = gated_scene();
    scene.session_operations.clear();

    let mut invalid = scene.clone();
    invalid.surfaces.clear();
    assert!(matches!(
        encode_wm_v1_policy_snapshot(TransactionId::from_raw(9), 2, &invalid, &[], &[], 0,),
        Err(IpcCodecError::InvalidEnum {
            field: "snapshot_output_focus",
            ..
        })
    ));

    let mut transfer =
        encode_wm_v1_policy_snapshot(TransactionId::from_raw(9), 2, &scene, &[], &[], 0).unwrap();
    transfer
        .chunks
        .retain(|chunk| chunk.record_kind != SNAPSHOT_SURFACE_RECORD_KIND);
    transfer.begin.surface_count = 0;
    transfer.begin.chunk_count = 1;
    transfer.end.chunk_count = 1;
    assert!(matches!(
        decode_wm_v1_policy_snapshot(&transfer),
        Err(IpcCodecError::InvalidEnum {
            field: "snapshot_output_focus",
            ..
        })
    ));
}

#[test]
fn complete_output_projection_roundtrips_in_stacking_order() {
    let proposal = PolicyProjectionProposal {
        presentation: None,
        launch_contexts: Vec::new(),
        output_launch_contexts: Vec::new(),
        translation_groups: Vec::new(),
        tab_groups: Vec::new(),
        transaction: TransactionId::from_raw(11),
        connection_epoch: 2,
        request_id: 5,
        base_generation: 7,
        active_output: OutputId::from_raw(1),
        outputs: vec![PolicyOutputProjection {
            output: OutputId::from_raw(1),
            placements: vec![PolicySurfacePlacement {
                surface: SurfaceId::new(3, 1),
                surface_generation: 8,
                geometry: Rect {
                    x: 20,
                    y: 30,
                    width: 800,
                    height: 600,
                },
                requested_size: Some(Size {
                    width: 800,
                    height: 600,
                }),
                crop: Some(Rect {
                    x: 1,
                    y: 2,
                    width: 799,
                    height: 598,
                }),
                transform: PolicyTransform::Identity,
                presentation: PolicyPresentationState {
                    fullscreen: true,
                    ..PolicyPresentationState::default()
                },
            }],
            focus: Some(SurfaceId::new(3, 1)),
        }],
        indicators: vec![PolicyProjectionIndicator {
            output: OutputId::from_raw(1),
            slot: 0,
            indicator: 1,
            action: Some(WmActionId::from_raw(11)),
            state_bits: POLICY_INDICATOR_STATE_ACTIVE,
            label: "1".into(),
        }],
        output_statuses: vec![PolicyProjectionOutputStatus {
            output: OutputId::from_raw(1),
            focus_bits: POLICY_OUTPUT_STATUS_HAS_FOCUSED_SURFACE,
            layout: "Scroller".into(),
        }],
    };
    let transfer = encode_wm_v1_policy_projection(&proposal).unwrap();

    assert_eq!(decode_wm_v1_policy_projection(&transfer), Ok(proposal));
    assert_eq!(transfer.chunks.len(), 4);
}

#[test]
fn projection_record_count_mismatch_fails_closed() {
    let proposal = PolicyProjectionProposal {
        presentation: None,
        launch_contexts: Vec::new(),
        output_launch_contexts: Vec::new(),
        translation_groups: Vec::new(),
        tab_groups: Vec::new(),
        transaction: TransactionId::from_raw(13),
        connection_epoch: 2,
        request_id: 5,
        base_generation: 7,
        active_output: OutputId::from_raw(1),
        outputs: vec![PolicyOutputProjection {
            output: OutputId::from_raw(1),
            placements: Vec::new(),
            focus: None,
        }],
        indicators: Vec::new(),
        output_statuses: Vec::new(),
    };
    let mut transfer = encode_wm_v1_policy_projection(&proposal).unwrap();
    transfer.begin.placement_count = 1;

    assert!(matches!(
        decode_wm_v1_policy_projection(&transfer),
        Err(IpcCodecError::InvalidEnum {
            field: "record_count",
            ..
        })
    ));
}

#[test]
fn projection_request_carries_the_complete_affected_output_set() {
    let request = PolicyProjectionRequest {
        connection_epoch: 2,
        request_id: 5,
        scene_generation: 7,
        policy_generation: 3,
        affected_outputs: vec![OutputId::from_raw(1), OutputId::from_raw(2)],
        cause: PolicyRequestCause::Action {
            activation_serial: 9,
            action: WmActionId::from_raw(4),
        },
    };
    let encoded = encode_wm_v1_policy_projection_request(&request).unwrap();

    assert_eq!(
        decode_wm_v1_policy_projection_request(&encoded),
        Ok(request)
    );
    assert_eq!(encoded.affected_outputs.len(), 16);
}

#[test]
fn projection_request_rejects_duplicate_or_truncated_output_ids() {
    let duplicate = PolicyProjectionRequest {
        connection_epoch: 2,
        request_id: 5,
        scene_generation: 7,
        policy_generation: 3,
        affected_outputs: vec![OutputId::from_raw(1), OutputId::from_raw(1)],
        cause: PolicyRequestCause::SceneChanged,
    };
    assert!(encode_wm_v1_policy_projection_request(&duplicate).is_err());

    let mut encoded = encode_wm_v1_policy_projection_request(&PolicyProjectionRequest {
        affected_outputs: vec![OutputId::from_raw(1)],
        ..duplicate
    })
    .unwrap();
    encoded.affected_outputs.pop();
    assert!(decode_wm_v1_policy_projection_request(&encoded).is_err());
}

#[test]
fn projection_outcome_has_one_strict_semantic_mapping() {
    for outcome in [
        PolicyProjectionOutcome::Committed,
        PolicyProjectionOutcome::RejectedStale,
        PolicyProjectionOutcome::RejectedInvalid,
        PolicyProjectionOutcome::TimedOut,
        PolicyProjectionOutcome::Disconnected,
    ] {
        let encoded = encode_wm_v1_policy_projection_outcome(2, 5, 7, outcome).unwrap();
        assert_eq!(
            decode_wm_v1_policy_projection_outcome(&encoded),
            Ok(outcome)
        );
    }
}

#[test]
fn configuration_dirty_and_session_operations_have_typed_mappings() {
    let configuration = PolicyConfiguration {
        connection_epoch: 2,
        generation: 3,
        actions: vec![PolicyActionRegistration {
            action: WmActionId::from_raw(7),
            name: "resize-width 0.1".to_owned(),
            session_operation_slot: Some(1),
        }],
        chrome: WmChromePolicy::default(),
    };
    let encoded = encode_wm_v1_policy_configuration(&configuration).unwrap();
    assert_eq!(
        decode_wm_v1_policy_configuration(&encoded),
        Ok(configuration)
    );

    let dirty = PolicyDirtyRequest {
        connection_epoch: 2,
        policy_generation: 4,
        affected_outputs: vec![OutputId::from_raw(1), OutputId::from_raw(2)],
    };
    let encoded = encode_wm_v1_policy_dirty(&dirty).unwrap();
    assert_eq!(decode_wm_v1_policy_dirty(&encoded), Ok(dirty));

    let operation = PolicySessionOperationRequest {
        connection_epoch: 2,
        request_id: 9,
        operation: 11,
        target: Some(SurfaceId::new(3, 1)),
    };
    let encoded = encode_wm_v1_policy_session_operation_request(operation).unwrap();
    assert_eq!(
        decode_wm_v1_policy_session_operation_request(&encoded),
        Ok(operation)
    );
}

#[test]
fn policy_configuration_rejects_ambiguous_or_invalid_actions() {
    let action = PolicyActionRegistration {
        action: WmActionId::from_raw(7),
        name: "resize-width 0.1".to_owned(),
        session_operation_slot: None,
    };
    let mut configuration = PolicyConfiguration {
        connection_epoch: 2,
        generation: 3,
        actions: vec![action.clone(), action],
        chrome: WmChromePolicy::default(),
    };
    assert!(encode_wm_v1_policy_configuration(&configuration).is_err());

    configuration.actions = vec![PolicyActionRegistration {
        action: WmActionId::from_raw(8),
        name: " leading-space".to_owned(),
        session_operation_slot: None,
    }];
    assert!(encode_wm_v1_policy_configuration(&configuration).is_err());
}

#[test]
fn reduced_interaction_preserves_kind_phase_and_geometry() {
    let request = PolicyProjectionRequest {
        connection_epoch: 2,
        request_id: 6,
        scene_generation: 7,
        policy_generation: 3,
        affected_outputs: vec![OutputId::from_raw(1)],
        cause: PolicyRequestCause::Interaction {
            phase: PolicyInteractionPhase::Cancel,
            kind: PolicyInteractionKind::Resize,
            axis: PolicyInteractionAxis::None,
            target: SurfaceId::new(3, 1),
            geometry: Rect {
                x: 20,
                y: 30,
                width: 800,
                height: 600,
            },
        },
    };

    let encoded = encode_wm_v1_policy_projection_request(&request).unwrap();
    assert_eq!(
        decode_wm_v1_policy_projection_request(&encoded),
        Ok(request)
    );
}

#[test]
fn revision_three_interaction_vocabulary_has_one_fixed_payload_contract() {
    let target = SurfaceId::new(3, 1);
    for kind in [
        PolicyInteractionKind::Move,
        PolicyInteractionKind::Resize,
        PolicyInteractionKind::Drag,
    ] {
        let request = PolicyProjectionRequest {
            connection_epoch: 2,
            request_id: 6,
            scene_generation: 7,
            policy_generation: 3,
            affected_outputs: vec![OutputId::from_raw(1)],
            cause: PolicyRequestCause::Interaction {
                phase: PolicyInteractionPhase::Update,
                kind,
                axis: PolicyInteractionAxis::None,
                target,
                geometry: Rect {
                    x: 20,
                    y: 30,
                    width: 800,
                    height: 600,
                },
            },
        };
        let encoded = encode_wm_v1_policy_projection_request(&request).unwrap();
        assert_eq!(
            decode_wm_v1_policy_projection_request(&encoded),
            Ok(request)
        );
    }

    for (phase, delta) in [
        (PolicyInteractionPhase::Begin, -120),
        (PolicyInteractionPhase::Update, -60),
        (PolicyInteractionPhase::End, -30),
        (PolicyInteractionPhase::Cancel, 0),
    ] {
        let request = PolicyProjectionRequest {
            connection_epoch: 2,
            request_id: 6,
            scene_generation: 7,
            policy_generation: 3,
            affected_outputs: vec![OutputId::from_raw(1)],
            cause: PolicyRequestCause::Interaction {
                phase,
                kind: PolicyInteractionKind::Scroll,
                axis: PolicyInteractionAxis::Vertical,
                target,
                geometry: Rect {
                    x: 0,
                    y: delta,
                    width: 0,
                    height: 0,
                },
            },
        };
        let encoded = encode_wm_v1_policy_projection_request(&request).unwrap();
        assert_eq!(
            decode_wm_v1_policy_projection_request(&encoded),
            Ok(request)
        );
    }
}

#[test]
fn revision_three_interaction_payload_rejects_ambiguous_encodings() {
    let target = SurfaceId::new(3, 1);
    let request = |phase, kind, axis, geometry| PolicyProjectionRequest {
        connection_epoch: 2,
        request_id: 6,
        scene_generation: 7,
        policy_generation: 3,
        affected_outputs: vec![OutputId::from_raw(1)],
        cause: PolicyRequestCause::Interaction {
            phase,
            kind,
            axis,
            target,
            geometry,
        },
    };

    for invalid in [
        request(
            PolicyInteractionPhase::Update,
            PolicyInteractionKind::Scroll,
            PolicyInteractionAxis::None,
            Rect {
                x: 0,
                y: -60,
                width: 0,
                height: 0,
            },
        ),
        request(
            PolicyInteractionPhase::Update,
            PolicyInteractionKind::Scroll,
            PolicyInteractionAxis::Vertical,
            Rect {
                x: 0,
                y: -60,
                width: 1,
                height: 0,
            },
        ),
        request(
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
        request(
            PolicyInteractionPhase::End,
            PolicyInteractionKind::Scroll,
            PolicyInteractionAxis::Horizontal,
            Rect::default(),
        ),
    ] {
        assert!(encode_wm_v1_policy_projection_request(&invalid).is_err());
    }

    let valid = request(
        PolicyInteractionPhase::Update,
        PolicyInteractionKind::Scroll,
        PolicyInteractionAxis::Vertical,
        Rect {
            x: 0,
            y: -60,
            width: 0,
            height: 0,
        },
    );
    let encoded = encode_wm_v1_policy_projection_request(&valid).unwrap();

    let mut unknown_axis = encoded.clone();
    unknown_axis.interaction_axis = 3;
    assert!(matches!(
        decode_wm_v1_policy_projection_request(&unknown_axis),
        Err(IpcCodecError::InvalidEnum {
            field: "interaction_axis",
            value: 3,
        })
    ));

    let mut unknown_kind = encoded;
    unknown_kind.interaction_kind = 5;
    assert!(matches!(
        decode_wm_v1_policy_projection_request(&unknown_kind),
        Err(IpcCodecError::InvalidEnum {
            field: "interaction_kind",
            value: 5,
        })
    ));
}

/// Pins what the producer emits for a given capability set.
///
/// The existing golden tests are round-trip identity checks: they prove the codec
/// re-encodes a recorded frame to itself, never that the compositor emits those
/// bytes. This test pins the producer instead, which is what makes the
/// forward-compatibility rule in `docs/sophia-policy-ipc.md` enforceable. Without
/// it, silently widening what the server sends a client that negotiated nothing
/// would go unnoticed until a frozen client rejected a transfer in the field.
#[test]
fn capability_gating_omits_ungated_content_without_perturbing_the_rest() {
    let scene = gated_scene();
    let actions = vec![PolicyActionRegistration {
        action: WmActionId::from_raw(4),
        name: "close-window".to_owned(),
        session_operation_slot: Some(1),
    }];

    let ungated =
        encode_wm_v1_policy_snapshot(TransactionId::from_raw(9), 2, &scene, &actions, &[], 0)
            .unwrap();
    let gated = encode_wm_v1_policy_snapshot(
        TransactionId::from_raw(9),
        2,
        &scene,
        &actions,
        &[],
        SOPHIA_WM_CAPABILITY_ACTIONS | SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
    )
    .unwrap();

    // A client that negotiated nothing receives only the core scene, and the
    // declared counts drop with the chunks so the transfer stays self-consistent.
    assert_eq!(ungated.chunks.len(), 2);
    assert_eq!(ungated.begin.action_count, 0);
    assert_eq!(ungated.begin.session_operation_count, 0);
    assert_eq!(ungated.begin.chunk_count, 2);
    assert_eq!(ungated.end.chunk_count, 2);

    // The same scene for a fully capable client carries both governed kinds.
    assert_eq!(gated.chunks.len(), 4);
    assert_eq!(gated.begin.action_count, 1);
    assert_eq!(gated.begin.session_operation_count, 1);

    // The producer pin: enabling a capability must not perturb ungated content.
    // Chunk ordinals and bytes for outputs and surfaces are identical, so adding a
    // gated record kind cannot shift what an existing client already parses.
    for (lhs, rhs) in ungated.chunks.iter().zip(gated.chunks.iter()) {
        assert_eq!(lhs.ordinal, rhs.ordinal);
        assert_eq!(lhs.record_kind, rhs.record_kind);
        assert_eq!(lhs.item_count, rhs.item_count);
        assert_eq!(lhs.data, rhs.data);
    }

    // Both transfers decode. The ungated one fails closed by omission rather than
    // by carrying a count it never satisfies.
    let decoded_ungated = decode_wm_v1_policy_snapshot(&ungated).unwrap();
    assert!(decoded_ungated.actions.is_empty());
    assert!(decoded_ungated.scene.session_operations.is_empty());
    assert_eq!(decoded_ungated.scene.outputs, scene.outputs);
    assert_eq!(decoded_ungated.scene.surfaces, scene.surfaces);

    let decoded_gated = decode_wm_v1_policy_snapshot(&gated).unwrap();
    assert_eq!(decoded_gated.scene, scene);
    assert_eq!(decoded_gated.actions, actions);
}

#[test]
fn launch_placement_is_an_uncounted_gated_extension() {
    let scene = gated_scene();
    let classifications = [PolicySurfaceClassification {
        surface: scene.surfaces[0].surface,
        classification: 2,
    }];
    let baseline =
        encode_wm_v1_policy_snapshot(TransactionId::from_raw(9), 2, &scene, &[], &[], 0).unwrap();
    let gated_off = encode_wm_v1_policy_snapshot(
        TransactionId::from_raw(9),
        2,
        &scene,
        &[],
        &classifications,
        0,
    )
    .unwrap();
    assert_eq!(gated_off, baseline);

    let gated = encode_wm_v1_policy_snapshot(
        TransactionId::from_raw(9),
        2,
        &scene,
        &[],
        &classifications,
        SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT,
    )
    .unwrap();
    assert_eq!(gated.begin.chunk_count, baseline.begin.chunk_count);
    assert_eq!(gated.end.chunk_count, baseline.end.chunk_count);
    assert_eq!(gated.chunks.len(), baseline.chunks.len() + 1);
    let extension = gated.chunks.last().unwrap();
    assert_eq!(extension.ordinal as usize, baseline.chunks.len());
    assert_eq!(
        extension.record_kind,
        SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND
    );
    assert_eq!(extension.item_count, 1);

    let decoded = decode_wm_v1_policy_snapshot(&gated).unwrap();
    assert_eq!(decoded.classifications, classifications);
}

/// Each governed capability is independently gated, so a client negotiating one
/// does not receive the other. Corpora pin the default set plus each capability in
/// isolation; they deliberately do not enumerate combinations.
#[test]
fn each_governed_capability_gates_only_its_own_record_kind() {
    let scene = gated_scene();
    let actions = vec![PolicyActionRegistration {
        action: WmActionId::from_raw(4),
        name: "close-window".to_owned(),
        session_operation_slot: Some(1),
    }];

    let actions_only = encode_wm_v1_policy_snapshot(
        TransactionId::from_raw(9),
        2,
        &scene,
        &actions,
        &[],
        SOPHIA_WM_CAPABILITY_ACTIONS,
    )
    .unwrap();
    assert_eq!(actions_only.begin.action_count, 1);
    assert_eq!(actions_only.begin.session_operation_count, 0);
    assert_eq!(actions_only.chunks.len(), 3);
    decode_wm_v1_policy_snapshot(&actions_only).unwrap();

    let operations_only = encode_wm_v1_policy_snapshot(
        TransactionId::from_raw(9),
        2,
        &scene,
        &actions,
        &[],
        SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
    )
    .unwrap();
    assert_eq!(operations_only.begin.action_count, 0);
    assert_eq!(operations_only.begin.session_operation_count, 1);
    assert_eq!(operations_only.chunks.len(), 3);
    decode_wm_v1_policy_snapshot(&operations_only).unwrap();
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

#[test]
fn launch_origin_extension_roundtrips_without_changing_frozen_counts() {
    use sophia_protocol::*;
    let scene = gated_scene();
    let context = PolicyLaunchContext {
        surface: scene.surfaces[0].surface,
        epoch: 2,
        token: 41,
    };
    let original =
        encode_wm_v1_policy_snapshot(TransactionId::from_raw(9), 2, &scene, &[], &[], 0).unwrap();
    let mut transfer = original.clone();
    append_wm_launch_origins(&mut transfer, &[context], 0).unwrap();
    assert_eq!(transfer, original);
    append_wm_launch_origins(
        &mut transfer,
        &[context],
        SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN,
    )
    .unwrap();
    assert_eq!(transfer.begin, original.begin);
    assert_eq!(transfer.end, original.end);
    assert_eq!(
        decode_wm_v1_policy_snapshot(&transfer)
            .unwrap()
            .launch_origins,
        vec![context]
    );
    let mut stale = context;
    stale.epoch = 1;
    assert!(
        append_wm_launch_origins(&mut transfer, &[stale], SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN)
            .is_err()
    );
    let last = transfer.chunks.last_mut().unwrap();
    last.data[0..4].copy_from_slice(&999_u32.to_le_bytes());
    assert!(decode_wm_v1_policy_snapshot(&transfer).is_err());
}

#[test]
fn launch_bookmarks_reject_ambiguous_identity_and_cross_chunk_duplicates() {
    use sophia_protocol::*;
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
    let mut chunks = encode_wm_launch_contexts(&[context], 2, 0).unwrap();
    assert!(encode_wm_launch_contexts(&[context], 3, 0).is_err());
    chunks.extend(encode_wm_launch_contexts(&[context], 2, 1).unwrap());
    assert!(decode_wm_launch_contexts(&chunks).is_err());
}
