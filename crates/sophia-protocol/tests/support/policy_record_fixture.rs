use sophia_protocol::*;
pub fn rect() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
    }
}
pub fn scene() -> PolicySceneSnapshot {
    PolicySceneSnapshot {
        generation: 7,
        active_output: OutputId::from_raw(1),
        outputs: vec![PolicyOutputSnapshot {
            policy_key: Some(22),
            output: OutputId::from_raw(1),
            generation: 3,
            focus: None,
            bounds: rect(),
            work_area: rect(),
        }],
        surfaces: vec![PolicySurfaceSnapshot {
            surface: SurfaceId::new(3, 1),
            generation: 8,
            current_output: Some(OutputId::from_raw(1)),
            kind: PolicySurfaceKind::Dialog,
            capabilities: LayoutNodeCapabilities::STANDARD_TOPLEVEL,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            exact_size: None,
            requested_state: PolicyPresentationState::default(),
            current_state: PolicyPresentationState::default(),
            transient_owner: None,
            geometry: rect(),
        }],
        session_operations: vec![PolicySessionOperation {
            token: 11,
            slot: 1,
            permits_surface_target: true,
        }],
    }
}
pub fn actions() -> Vec<PolicyActionRegistration> {
    vec![PolicyActionRegistration {
        action: WmActionId::from_raw(5),
        name: "confirm".into(),
        session_operation_slot: None,
    }]
}
pub fn classifications() -> Vec<PolicySurfaceClassification> {
    vec![PolicySurfaceClassification {
        surface: SurfaceId::new(3, 1),
        classification: 9,
    }]
}
pub fn origins() -> Vec<PolicyLaunchContext> {
    vec![PolicyLaunchContext {
        surface: SurfaceId::new(3, 1),
        epoch: 2,
        token: 41,
    }]
}
pub fn proposal() -> PolicyProjectionProposal {
    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(3, 1);
    PolicyProjectionProposal {
        transaction: TransactionId::from_raw(11),
        connection_epoch: 2,
        request_id: 5,
        base_generation: 7,
        active_output: output,
        outputs: vec![PolicyOutputProjection {
            output,
            placements: vec![PolicySurfacePlacement {
                surface,
                surface_generation: 8,
                geometry: rect(),
                requested_size: None,
                crop: None,
                transform: PolicyTransform::Identity,
                presentation: PolicyPresentationState::default(),
            }],
            focus: Some(surface),
        }],
        indicators: vec![PolicyProjectionIndicator {
            output,
            slot: 0,
            indicator: 1,
            action: Some(WmActionId::from_raw(5)),
            state_bits: 0,
            label: "one".into(),
        }],
        output_statuses: vec![PolicyProjectionOutputStatus {
            output,
            focus_bits: 0,
            layout: "Scroller".into(),
        }],
        tab_groups: vec![PolicyTabGroup {
            output,
            group: 1,
            geometry: rect(),
            selected: Some(surface),
            focused: true,
            members: vec![surface],
        }],
        translation_groups: vec![PolicyTranslationGroup {
            output,
            group: 2,
            x: 5,
            y: 7,
            members: vec![surface],
        }],
        launch_contexts: origins(),
        output_launch_contexts: vec![PolicyOutputLaunchContext {
            output,
            output_generation: 3,
            epoch: 2,
            token: 51,
        }],
        presentation: Some(PolicyPresentation {
            generation: 4,
            keyboard_output: Some(output),
            outputs: vec![PolicyPresentationOutput {
                output,
                generation: 3,
                coverage: rect(),
                mode: PolicyPresentationMode::ReplaceApplications,
            }],
            instances: vec![PolicySurfaceInstance {
                id: 2,
                generation: 1,
                output,
                source: surface,
                destination: rect(),
                clip: rect(),
                opacity_millis: 1000,
                z_index: 1,
                action: Some(WmActionId::from_raw(5)),
            }],
            regions: vec![PolicyPresentationRegion {
                id: 1,
                generation: 1,
                output,
                geometry: rect(),
                clip: rect(),
                z_index: 0,
                role: PolicyPresentationRegionRole::Backdrop,
                action: None,
            }],
            bindings: vec![PolicyPresentationBinding {
                action: WmActionId::from_raw(5),
                keycode: 28,
                modifiers: WmModifierMask { bits: 0 },
            }],
        }),
    }
}
pub fn legacy_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for capabilities in [0, u64::MAX] {
        let mut snapshot = encode_wm_v1_policy_snapshot(
            TransactionId::from_raw(9),
            2,
            &scene(),
            &actions(),
            &classifications(),
            capabilities,
        )
        .unwrap();
        append_wm_launch_origins(&mut snapshot, &origins(), capabilities).unwrap();
        bytes.extend(
            encode_wm_v1_snapshot_begin_frame(snapshot.transaction, &snapshot.begin).unwrap(),
        );
        for c in &snapshot.chunks {
            bytes.extend(encode_wm_v1_snapshot_chunk_frame(snapshot.transaction, c).unwrap());
        }
        bytes.extend(encode_wm_v1_snapshot_end_frame(snapshot.transaction, &snapshot.end).unwrap());
    }
    for large in [false, true] {
        let mut p = proposal();
        if large {
            let scene = p.presentation.as_mut().unwrap();
            let instance = scene.instances[0];
            scene.instances = (0..1024)
                .map(|i| PolicySurfaceInstance {
                    id: i + 2,
                    z_index: i as u16 + 1,
                    ..instance
                })
                .collect();
        }
        let projection = encode_wm_v1_policy_projection(&p).unwrap();
        bytes.extend(
            encode_wm_v1_projection_begin_frame(projection.transaction, &projection.begin).unwrap(),
        );
        for c in &projection.chunks {
            bytes.extend(encode_wm_v1_projection_chunk_frame(projection.transaction, c).unwrap());
        }
        bytes.extend(
            encode_wm_v1_projection_end_frame(projection.transaction, &projection.end).unwrap(),
        );
    }
    bytes
}
