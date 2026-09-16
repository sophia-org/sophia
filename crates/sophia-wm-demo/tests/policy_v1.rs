use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sophia_protocol::{
    LayoutNodeCapabilities, OutputId, PolicyActionRegistration, PolicyOutputSnapshot,
    PolicyPresentationState, PolicyProjectionRequest, PolicyRequestCause, PolicySceneSnapshot,
    PolicySurfaceKind, PolicySurfaceSnapshot, Rect, SOPHIA_IPC_HEADER_LEN,
    SOPHIA_WM_CAPABILITY_CONFIGURATION, SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
    SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS, SOPHIA_WM_INTERFACE_REVISION,
    SOPHIA_WM_OUTCOME_COMMITTED, Size, SurfaceConstraints, SurfaceId, TransactionId,
    WM_V1_PROFILE_DIGEST_BYTES, WmActionId, WmChromePolicy, WmV1PolicyConfigurationOutcome,
    WmV1ProfileCommand, WmV1ProfileIdentity, WmV1ServerWelcome, WmV1SessionOperationOutcome,
    decode_wm_v1_client_hello_frame, decode_wm_v1_policy_configuration,
    decode_wm_v1_policy_configuration_frame, decode_wm_v1_policy_session_operation_request,
    decode_wm_v1_profile_active, decode_wm_v1_profile_prepared,
    decode_wm_v1_session_operation_request_frame, encode_wm_v1_policy_configuration_outcome_frame,
    encode_wm_v1_profile_activate, encode_wm_v1_profile_prepare, encode_wm_v1_server_welcome_frame,
    encode_wm_v1_session_operation_outcome_frame,
};
use sophia_wm_demo::{
    PolicyV1Client, StatelessReferenceProjectionDecision, partition_policy_scene_across_outputs,
    policy_projection_distinct_surface_count, stateless_reference_projection_decision,
    tile_policy_scene,
};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);

#[test]
fn stateless_reference_recovers_from_a_moved_scene_and_fails_only_on_its_own_faults() {
    // A stale generation and a timeout both mean the scene moved on; the
    // session preserves the committed layout either way, so re-proposing from
    // a fresh snapshot is the entire recovery. A physical run treated the
    // timeout as fatal, which let one slow client exit the window manager
    // three times over and exhaust its restart budget.
    for outcome in [
        sophia_protocol::PolicyProjectionOutcome::RejectedStale,
        sophia_protocol::PolicyProjectionOutcome::TimedOut,
    ] {
        assert_eq!(
            stateless_reference_projection_decision(outcome),
            StatelessReferenceProjectionDecision::RetryFreshSnapshot
        );
    }
    assert_eq!(
        stateless_reference_projection_decision(
            sophia_protocol::PolicyProjectionOutcome::Committed,
        ),
        StatelessReferenceProjectionDecision::Settled
    );
    // These name a fault in the proposal or the connection itself, which
    // retrying cannot repair.
    for outcome in [
        sophia_protocol::PolicyProjectionOutcome::RejectedInvalid,
        sophia_protocol::PolicyProjectionOutcome::Disconnected,
    ] {
        assert_eq!(
            stateless_reference_projection_decision(outcome),
            StatelessReferenceProjectionDecision::Fatal
        );
    }
}

#[test]
fn reference_policy_adopts_unassigned_surfaces_on_the_active_affected_output() {
    let output = OutputId::from_raw(1);
    let mut minimized = surface(4, None);
    minimized.current_state.minimized = true;
    let scene = PolicySceneSnapshot {
        generation: 4,
        active_output: output,
        outputs: vec![PolicyOutputSnapshot {
            policy_key: None,
            output,
            generation: 1,
            focus: Some(SurfaceId::new(1, 1)),
            bounds: Rect {
                x: 10,
                y: 20,
                width: 101,
                height: 80,
            },
            work_area: Rect {
                x: 10,
                y: 20,
                width: 101,
                height: 80,
            },
        }],
        surfaces: vec![
            surface(1, Some(output)),
            surface(2, Some(output)),
            surface(3, None),
            minimized,
        ],
        session_operations: Vec::new(),
    };
    let request = PolicyProjectionRequest {
        connection_epoch: 7,
        request_id: 8,
        scene_generation: 4,
        policy_generation: 1,
        affected_outputs: vec![output],
        cause: PolicyRequestCause::SceneChanged,
    };

    let proposal = tile_policy_scene(TransactionId::from_raw(9), &scene, &request).unwrap();

    assert_eq!(proposal.outputs.len(), 1);
    assert_eq!(proposal.outputs[0].placements.len(), 3);
    assert_eq!(proposal.outputs[0].placements[0].geometry.width, 33);
    assert_eq!(proposal.outputs[0].placements[1].geometry.width, 33);
    assert_eq!(proposal.outputs[0].placements[2].geometry.width, 35);
    assert_eq!(proposal.outputs[0].focus, Some(SurfaceId::new(1, 1)));
}

#[test]
fn mixed_output_start_barrier_uses_the_committed_proposal_without_an_echo_cycle() {
    let output = OutputId::from_raw(1);
    let scene = PolicySceneSnapshot {
        generation: 4,
        active_output: output,
        outputs: vec![PolicyOutputSnapshot {
            policy_key: None,
            output,
            generation: 1,
            focus: None,
            bounds: Rect {
                x: 0,
                y: 0,
                width: 2_560,
                height: 1_440,
            },
            work_area: Rect {
                x: 0,
                y: 0,
                width: 2_560,
                height: 1_440,
            },
        }],
        surfaces: vec![surface(1, None), surface(2, None)],
        session_operations: Vec::new(),
    };
    let request = PolicyProjectionRequest {
        connection_epoch: 7,
        request_id: 8,
        scene_generation: 4,
        policy_generation: 1,
        affected_outputs: vec![output],
        cause: PolicyRequestCause::SceneChanged,
    };

    let proposal = tile_policy_scene(TransactionId::from_raw(9), &scene, &request).unwrap();

    assert_eq!(
        scene
            .surfaces
            .iter()
            .filter(|surface| surface.current_output.is_some())
            .count(),
        0
    );
    assert_eq!(policy_projection_distinct_surface_count(&proposal), 2);
}

#[test]
fn reference_policy_never_duplicates_unassigned_surfaces_across_outputs() {
    let left = OutputId::from_raw(1);
    let right = OutputId::from_raw(2);
    let scene = PolicySceneSnapshot {
        generation: 4,
        active_output: right,
        outputs: vec![
            PolicyOutputSnapshot {
                policy_key: None,
                output: left,
                generation: 1,
                focus: None,
                bounds: Rect {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 80,
                },
                work_area: Rect {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 80,
                },
            },
            PolicyOutputSnapshot {
                policy_key: None,
                output: right,
                generation: 1,
                focus: None,
                bounds: Rect {
                    x: 100,
                    y: 0,
                    width: 100,
                    height: 80,
                },
                work_area: Rect {
                    x: 100,
                    y: 0,
                    width: 100,
                    height: 80,
                },
            },
        ],
        surfaces: vec![surface(1, Some(left)), surface(2, None)],
        session_operations: Vec::new(),
    };
    let request = PolicyProjectionRequest {
        connection_epoch: 7,
        request_id: 8,
        scene_generation: 4,
        policy_generation: 1,
        affected_outputs: vec![left, right],
        cause: PolicyRequestCause::SceneChanged,
    };

    let proposal = tile_policy_scene(TransactionId::from_raw(9), &scene, &request).unwrap();

    assert_eq!(proposal.outputs[0].placements.len(), 1);
    assert_eq!(
        proposal.outputs[0].placements[0].surface,
        SurfaceId::new(1, 1)
    );
    assert_eq!(proposal.outputs[1].placements.len(), 1);
    assert_eq!(
        proposal.outputs[1].placements[0].surface,
        SurfaceId::new(2, 1)
    );
}

#[test]
fn mixed_proof_partitions_surfaces_by_logical_geometry_without_head_identity() {
    let left = OutputId::from_raw(1);
    let right = OutputId::from_raw(2);
    let mut scene = PolicySceneSnapshot {
        generation: 4,
        active_output: left,
        outputs: vec![
            PolicyOutputSnapshot {
                policy_key: None,
                output: right,
                generation: 1,
                focus: None,
                bounds: Rect {
                    x: 2560,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                work_area: Rect {
                    x: 2560,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
            },
            PolicyOutputSnapshot {
                policy_key: None,
                output: left,
                generation: 1,
                focus: Some(SurfaceId::new(1, 1)),
                bounds: Rect {
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1440,
                },
                work_area: Rect {
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1440,
                },
            },
        ],
        surfaces: vec![surface(1, Some(left)), surface(2, Some(left))],
        session_operations: Vec::new(),
    };

    partition_policy_scene_across_outputs(&mut scene).unwrap();

    assert_eq!(scene.active_output, right);
    assert_eq!(scene.surfaces[0].current_output, Some(left));
    assert_eq!(scene.surfaces[1].current_output, Some(right));
}

#[test]
fn live_reference_policy_accepts_profile_and_configures_before_scene_intake() {
    let socket = std::env::temp_dir().join(format!(
        "sophia-wm-demo-policy-v1-profile-{}-{}",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let hello = decode_wm_v1_client_hello_frame(&read_frame(&mut stream)).unwrap();
        assert_ne!(
            hello.capabilities & SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
            0
        );
        assert_ne!(hello.capabilities & SOPHIA_WM_CAPABILITY_CONFIGURATION, 0);
        stream
            .write_all(
                &encode_wm_v1_server_welcome_frame(&WmV1ServerWelcome {
                    selected_revision: SOPHIA_WM_INTERFACE_REVISION,
                    capabilities: hello.capabilities,
                    connection_epoch: 7,
                    max_outputs: 16,
                    max_bindings: 256,
                    max_surfaces: 1024,
                    max_chunk_bytes: 65_520,
                })
                .unwrap(),
            )
            .unwrap();
        let identity = WmV1ProfileIdentity::new(7, 4, [0x5a; WM_V1_PROFILE_DIGEST_BYTES]).unwrap();
        stream
            .write_all(
                &encode_wm_v1_profile_prepare(WmV1ProfileCommand {
                    transaction: TransactionId::from_raw(1),
                    identity,
                })
                .unwrap(),
            )
            .unwrap();
        let prepared = decode_wm_v1_profile_prepared(&read_frame(&mut stream)).unwrap();
        assert_eq!(prepared.identity, identity);
        stream
            .write_all(
                &encode_wm_v1_profile_activate(WmV1ProfileCommand {
                    transaction: TransactionId::from_raw(2),
                    identity,
                })
                .unwrap(),
            )
            .unwrap();
        let active = decode_wm_v1_profile_active(&read_frame(&mut stream)).unwrap();
        assert_eq!(active.identity, identity);
        let (transaction, wire) =
            decode_wm_v1_policy_configuration_frame(&read_frame(&mut stream)).unwrap();
        let configuration = decode_wm_v1_policy_configuration(&wire).unwrap();
        assert_eq!(configuration.connection_epoch, 7);
        assert_eq!(configuration.generation, 1);
        assert!(configuration.actions.is_empty());
        stream
            .write_all(
                &encode_wm_v1_policy_configuration_outcome_frame(
                    transaction,
                    &WmV1PolicyConfigurationOutcome {
                        connection_epoch: 7,
                        configuration_generation: 1,
                        outcome: SOPHIA_WM_OUTCOME_COMMITTED,
                    },
                )
                .unwrap(),
            )
            .unwrap();
    });

    let mut client = PolicyV1Client::connect(&socket, Duration::from_secs(1)).unwrap();
    client.activate_profile_and_configure().unwrap();

    server.join().unwrap();
    std::fs::remove_file(socket).unwrap();
}

#[test]
fn configured_reference_policy_requests_one_advertised_session_operation() {
    let socket = std::env::temp_dir().join(format!(
        "sophia-wm-demo-policy-v1-operation-{}-{}",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let hello = decode_wm_v1_client_hello_frame(&read_frame(&mut stream)).unwrap();
        assert_ne!(
            hello.capabilities & SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
            0
        );
        stream
            .write_all(
                &encode_wm_v1_server_welcome_frame(&WmV1ServerWelcome {
                    selected_revision: SOPHIA_WM_INTERFACE_REVISION,
                    capabilities: hello.capabilities,
                    connection_epoch: 9,
                    max_outputs: 16,
                    max_bindings: 256,
                    max_surfaces: 1024,
                    max_chunk_bytes: 65_520,
                })
                .unwrap(),
            )
            .unwrap();
        let identity = WmV1ProfileIdentity::new(9, 1, [0x3c; WM_V1_PROFILE_DIGEST_BYTES]).unwrap();
        stream
            .write_all(
                &encode_wm_v1_profile_prepare(WmV1ProfileCommand {
                    transaction: TransactionId::from_raw(1),
                    identity,
                })
                .unwrap(),
            )
            .unwrap();
        let _ = decode_wm_v1_profile_prepared(&read_frame(&mut stream)).unwrap();
        stream
            .write_all(
                &encode_wm_v1_profile_activate(WmV1ProfileCommand {
                    transaction: TransactionId::from_raw(2),
                    identity,
                })
                .unwrap(),
            )
            .unwrap();
        let _ = decode_wm_v1_profile_active(&read_frame(&mut stream)).unwrap();

        let (configuration_transaction, wire) =
            decode_wm_v1_policy_configuration_frame(&read_frame(&mut stream)).unwrap();
        let configuration = decode_wm_v1_policy_configuration(&wire).unwrap();
        assert_eq!(configuration.actions.len(), 1);
        assert_eq!(configuration.actions[0].name, "spawn-terminal");
        assert_eq!(configuration.actions[0].session_operation_slot, Some(1));
        stream
            .write_all(
                &encode_wm_v1_policy_configuration_outcome_frame(
                    configuration_transaction,
                    &WmV1PolicyConfigurationOutcome {
                        connection_epoch: 9,
                        configuration_generation: 1,
                        outcome: SOPHIA_WM_OUTCOME_COMMITTED,
                    },
                )
                .unwrap(),
            )
            .unwrap();

        let (operation_transaction, wire) =
            decode_wm_v1_session_operation_request_frame(&read_frame(&mut stream)).unwrap();
        let operation = decode_wm_v1_policy_session_operation_request(&wire).unwrap();
        assert_eq!(operation.connection_epoch, 9);
        assert_eq!(operation.operation, 77);
        assert_eq!(operation.target, Some(SurfaceId::new(4, 2)));
        stream
            .write_all(
                &encode_wm_v1_session_operation_outcome_frame(
                    operation_transaction,
                    &WmV1SessionOperationOutcome {
                        connection_epoch: 9,
                        request_id: operation.request_id,
                        outcome: SOPHIA_WM_OUTCOME_COMMITTED,
                    },
                )
                .unwrap(),
            )
            .unwrap();
    });

    let mut client = PolicyV1Client::connect(&socket, Duration::from_secs(1)).unwrap();
    client
        .activate_profile_and_configure_with(
            vec![PolicyActionRegistration {
                action: WmActionId::from_raw(0x300),
                name: "spawn-terminal".to_owned(),
                session_operation_slot: Some(1),
            }],
            WmChromePolicy::default(),
        )
        .unwrap();
    let outcome = client
        .request_session_operation(77, Some(SurfaceId::new(4, 2)))
        .unwrap();
    assert_eq!(
        outcome.outcome,
        sophia_protocol::PolicyProjectionOutcome::Committed
    );

    server.join().unwrap();
    std::fs::remove_file(socket).unwrap();
}

fn surface(index: u32, current_output: Option<OutputId>) -> PolicySurfaceSnapshot {
    PolicySurfaceSnapshot {
        surface: SurfaceId::new(index, 1),
        generation: 1,
        current_output,
        kind: PolicySurfaceKind::Toplevel,
        capabilities: LayoutNodeCapabilities::STANDARD_TOPLEVEL,
        constraints: SurfaceConstraints {
            min_size: Some(Size {
                width: 10,
                height: 10,
            }),
            max_size: None,
        },
        exact_size: None,
        requested_state: PolicyPresentationState::default(),
        current_state: PolicyPresentationState::default(),
        transient_owner: None,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 40,
        },
    }
}

fn read_frame(stream: &mut impl Read) -> Vec<u8> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header).unwrap();
    let payload_len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    let mut frame = header.to_vec();
    frame.resize(SOPHIA_IPC_HEADER_LEN + payload_len, 0);
    stream
        .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    frame
}
