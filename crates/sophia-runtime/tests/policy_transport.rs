#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sophia_protocol::{
    LayoutNodeCapabilities, OutputId, PolicyOutputSnapshot, PolicyPresentationState,
    PolicyProjectionOutcome, PolicyProjectionRequest, PolicyRequestCause, PolicySceneSnapshot,
    PolicySurfaceKind, PolicySurfaceSnapshot, Rect, SOPHIA_IPC_HEADER_LEN,
    SOPHIA_WM_CAPABILITY_ACTIONS, SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
    SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS, Size, SurfaceConstraints, SurfaceId, TransactionId,
    WM_V1_PROFILE_DIGEST_BYTES, WmV1ClientHello, WmV1ProfileCompletion, WmV1ProfileIdentity,
    WmV1ProfileOutcome, decode_frame, decode_wm_v1_policy_projection,
    decode_wm_v1_profile_activate, decode_wm_v1_profile_prepare, decode_wm_v1_profile_rollback,
    decode_wm_v1_server_welcome_frame, encode_wm_v1_client_hello_frame,
    encode_wm_v1_policy_snapshot, encode_wm_v1_profile_active, encode_wm_v1_profile_prepared,
    encode_wm_v1_profile_rolled_back,
};
use sophia_runtime::{
    PolicyPeerIdentity, PolicyProfileCompletionDisposition, PolicyProfileHandoffKind,
    PolicyProfileHandoffPhase, PolicyTransportError, PolicyWmSessionTransport,
    QueuedPolicyProjection,
};
use sophia_wm_demo::PolicyV1Client;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

#[test]
fn session_and_reference_client_exchange_one_complete_policy_cycle() {
    let directory = std::env::temp_dir().join(format!(
        "sophia-policy-transport-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let peer = PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    };
    let mut transport = PolicyWmSessionTransport::bind(&directory, peer).unwrap();
    let socket_path = transport.socket_path().to_path_buf();
    let client = std::thread::spawn(move || {
        let mut client = PolicyV1Client::connect(socket_path, Duration::from_secs(1)).unwrap();
        let snapshot = client.receive_snapshot().unwrap();
        let request = client.receive_projection_request().unwrap();
        let proposal = client.tile_once(&snapshot.scene, &request).unwrap();
        client.send_projection(&proposal).unwrap();
        assert_eq!(
            client.receive_projection_outcome(&proposal).unwrap(),
            PolicyProjectionOutcome::Committed
        );
        proposal
    });

    transport
        .accept_and_negotiate(1, Duration::from_secs(1))
        .unwrap();
    let scene = scene();
    let snapshot = encode_wm_v1_policy_snapshot(
        TransactionId::from_raw(29),
        1,
        &scene,
        &[],
        &[],
        SOPHIA_WM_CAPABILITY_ACTIONS | SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
    )
    .unwrap();
    transport
        .send_snapshot(
            snapshot.transaction,
            &snapshot.begin,
            &snapshot.chunks,
            &snapshot.end,
        )
        .unwrap();
    transport
        .send_projection_request(
            TransactionId::from_raw(30),
            &PolicyProjectionRequest {
                connection_epoch: 1,
                request_id: 1,
                scene_generation: 1,
                policy_generation: 1,
                affected_outputs: vec![OutputId::from_raw(1)],
                cause: PolicyRequestCause::SceneChanged,
            },
        )
        .unwrap();
    assert_eq!(transport.receive_projection_part().unwrap(), None);
    assert_eq!(transport.receive_projection_part().unwrap(), None);
    assert_eq!(transport.receive_projection_part().unwrap(), None);
    let assembled = match transport.receive_projection_part().unwrap() {
        Some(QueuedPolicyProjection::Admitted(projection)) => projection,
        other => panic!("expected admitted projection, got {other:?}"),
    };
    transport
        .send_projection_outcome(
            assembled.transaction,
            assembled.request_id,
            assembled.base_generation,
            PolicyProjectionOutcome::Committed,
        )
        .unwrap();
    let expected = client.join().unwrap();
    assert_eq!(
        decode_wm_v1_policy_projection(&assembled.into_wire_transfer()),
        Ok(expected)
    );
    transport.disconnect().unwrap();
}

#[test]
fn startup_profile_transport_drives_exact_prepare_activate_and_rollback() {
    let directory = std::env::temp_dir().join(format!(
        "sophia-policy-profile-transport-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let peer = PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    };
    let mut transport =
        PolicyWmSessionTransport::bind_for_startup_profile_activation(&directory, peer).unwrap();
    let socket_path = transport.socket_path().to_path_buf();
    let client = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(socket_path).unwrap();
        let hello = encode_wm_v1_client_hello_frame(&WmV1ClientHello {
            minimum_revision: 3,
            maximum_revision: 3,
            capabilities: SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
        })
        .unwrap();
        stream.write_all(&hello).unwrap();
        let welcome = decode_wm_v1_server_welcome_frame(&read_frame(&mut stream)).unwrap();
        assert_eq!(
            welcome.capabilities,
            SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION
        );

        for kind in [
            PolicyProfileHandoffKind::Prepare,
            PolicyProfileHandoffKind::Activate,
            PolicyProfileHandoffKind::Rollback,
        ] {
            let frame = read_frame(&mut stream);
            let command = match kind {
                PolicyProfileHandoffKind::Prepare => decode_wm_v1_profile_prepare(&frame).unwrap(),
                PolicyProfileHandoffKind::Activate => {
                    decode_wm_v1_profile_activate(&frame).unwrap()
                }
                PolicyProfileHandoffKind::Rollback => {
                    decode_wm_v1_profile_rollback(&frame).unwrap()
                }
            };
            let completion = WmV1ProfileCompletion {
                transaction: command.transaction,
                identity: command.identity,
                outcome: WmV1ProfileOutcome::Accepted,
            };
            let frame = match kind {
                PolicyProfileHandoffKind::Prepare => {
                    encode_wm_v1_profile_prepared(completion).unwrap()
                }
                PolicyProfileHandoffKind::Activate => {
                    encode_wm_v1_profile_active(completion).unwrap()
                }
                PolicyProfileHandoffKind::Rollback => {
                    encode_wm_v1_profile_rolled_back(completion).unwrap()
                }
            };
            stream.write_all(&frame).unwrap();
        }
    });

    transport
        .accept_and_negotiate(9, Duration::from_secs(1))
        .unwrap();
    let identity = WmV1ProfileIdentity::new(9, 7, [0x5a; WM_V1_PROFILE_DIGEST_BYTES]).unwrap();
    let mut model = transport
        .activate_profile_handoff(
            identity,
            TransactionId::from_raw(1),
            TransactionId::from_raw(2),
        )
        .unwrap();
    assert_eq!(model.phase(), PolicyProfileHandoffPhase::Active);
    {
        let (kind, transaction) = (PolicyProfileHandoffKind::Rollback, 3);
        let settled = transport
            .execute_profile_handoff_step(&model, kind, TransactionId::from_raw(transaction))
            .unwrap();
        assert_eq!(
            settled.completion,
            Some(PolicyProfileCompletionDisposition::Accepted)
        );
        model = settled.model;
    }
    assert_eq!(model.phase(), PolicyProfileHandoffPhase::RolledBack);
    transport.disconnect().unwrap();
    client.join().unwrap();
}

#[test]
fn profile_activation_rejects_an_out_of_phase_completion() {
    assert_eq!(
        profile_activation_error(ProfileTestResponse::Active(WmV1ProfileOutcome::Accepted)),
        PolicyTransportError::ProfileCompletionOutOfPhase
    );
}

#[test]
fn profile_activation_preserves_a_typed_identity_rejection() {
    assert_eq!(
        profile_activation_error(ProfileTestResponse::Prepared(
            WmV1ProfileOutcome::RejectedIdentity
        )),
        PolicyTransportError::ProfileRejected {
            kind: PolicyProfileHandoffKind::Prepare,
            outcome: WmV1ProfileOutcome::RejectedIdentity,
        }
    );
}

#[derive(Clone, Copy)]
enum ProfileTestResponse {
    Prepared(WmV1ProfileOutcome),
    Active(WmV1ProfileOutcome),
}

fn profile_activation_error(response: ProfileTestResponse) -> PolicyTransportError {
    let directory = std::env::temp_dir().join(format!(
        "sophia-policy-profile-rejection-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let peer = PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    };
    let mut transport =
        PolicyWmSessionTransport::bind_for_startup_profile_activation(&directory, peer).unwrap();
    let socket_path = transport.socket_path().to_path_buf();
    let client = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(socket_path).unwrap();
        stream
            .write_all(
                &encode_wm_v1_client_hello_frame(&WmV1ClientHello {
                    minimum_revision: 3,
                    maximum_revision: 3,
                    capabilities: SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
                })
                .unwrap(),
            )
            .unwrap();
        decode_wm_v1_server_welcome_frame(&read_frame(&mut stream)).unwrap();
        let command = decode_wm_v1_profile_prepare(&read_frame(&mut stream)).unwrap();
        let completion = WmV1ProfileCompletion {
            transaction: command.transaction,
            identity: command.identity,
            outcome: match response {
                ProfileTestResponse::Prepared(outcome) | ProfileTestResponse::Active(outcome) => {
                    outcome
                }
            },
        };
        let frame = match response {
            ProfileTestResponse::Prepared(_) => encode_wm_v1_profile_prepared(completion),
            ProfileTestResponse::Active(_) => encode_wm_v1_profile_active(completion),
        }
        .unwrap();
        stream.write_all(&frame).unwrap();
    });

    transport
        .accept_and_negotiate(9, Duration::from_secs(1))
        .unwrap();
    let identity = WmV1ProfileIdentity::new(9, 7, [0x5a; WM_V1_PROFILE_DIGEST_BYTES]).unwrap();
    let error = transport
        .activate_profile_handoff(
            identity,
            TransactionId::from_raw(1),
            TransactionId::from_raw(2),
        )
        .unwrap_err();
    transport.disconnect().unwrap();
    client.join().unwrap();
    error
}

fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut header = [0_u8; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header).unwrap();
    let payload_len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    let mut frame = header.to_vec();
    frame.resize(SOPHIA_IPC_HEADER_LEN + payload_len, 0);
    stream
        .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    decode_frame(&frame).unwrap();
    frame
}

fn scene() -> PolicySceneSnapshot {
    let output = OutputId::from_raw(1);
    PolicySceneSnapshot {
        generation: 1,
        active_output: output,
        outputs: vec![PolicyOutputSnapshot {
            policy_key: None,
            output,
            generation: 1,
            focus: Some(SurfaceId::new(3, 1)),
            bounds: Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
            work_area: Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
        }],
        surfaces: vec![PolicySurfaceSnapshot {
            surface: SurfaceId::new(3, 1),
            generation: 1,
            current_output: Some(output),
            kind: PolicySurfaceKind::Toplevel,
            capabilities: LayoutNodeCapabilities::STANDARD_TOPLEVEL,
            constraints: SurfaceConstraints {
                min_size: Some(Size {
                    width: 20,
                    height: 20,
                }),
                max_size: None,
            },
            exact_size: None,
            requested_state: PolicyPresentationState::default(),
            current_state: PolicyPresentationState::default(),
            transient_owner: None,
            geometry: Rect {
                x: 10,
                y: 10,
                width: 40,
                height: 40,
            },
        }],
        session_operations: Vec::new(),
    }
}

/// A socket timeout is not a broken transport.
///
/// The policy socket carries `SO_RCVTIMEO`, and Linux reports its expiry as
/// `WouldBlock`. Folded into an `Io` error it read as a dead peer and
/// restarted a window manager that was merely slow, which is exactly what a
/// client is after a topology change hands it a whole new layout to compute.
#[test]
fn a_silent_client_times_out_rather_than_failing_the_transport() {
    let directory = std::env::temp_dir().join(format!(
        "sophia-policy-transport-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let peer = PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    };
    let mut transport = PolicyWmSessionTransport::bind(&directory, peer).unwrap();
    let socket_path = transport.socket_path().to_path_buf();
    // Connects, negotiates, then says nothing at all.
    let client = std::thread::spawn(move || {
        let client = PolicyV1Client::connect(socket_path, Duration::from_secs(1)).unwrap();
        std::thread::sleep(Duration::from_millis(600));
        drop(client);
    });

    transport
        .accept_and_negotiate(1, Duration::from_millis(100))
        .unwrap();

    // One expired window is a timeout, not a fault: the connection is intact
    // and the caller may wait again.
    assert_eq!(
        transport.receive_client_event(),
        Err(PolicyTransportError::TimedOut)
    );

    // Waiting across several windows is still bounded, and still a timeout
    // rather than a transport failure.
    assert_eq!(
        transport.receive_client_event_within(Duration::from_millis(250)),
        Err(PolicyTransportError::TimedOut)
    );

    // A non-blocking poll leaves the socket usable, which it did not when it
    // returned early without restoring blocking mode.
    assert_eq!(transport.try_receive_client_event(), Ok(None));
    assert_eq!(
        transport.receive_client_event(),
        Err(PolicyTransportError::TimedOut)
    );

    client.join().unwrap();
}

#[test]
fn pointer_focus_is_sent_only_to_a_peer_that_negotiated_it() {
    use sophia_protocol::{
        SOPHIA_WM_CAPABILITY_POINTER_FOCUS, decode_wm_v1_projection_request_frame,
    };
    for enabled in [false, true] {
        let directory = std::env::temp_dir().join(format!(
            "sophia-pointer-focus-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = PolicyWmSessionTransport::bind(
            &directory,
            PolicyPeerIdentity {
                uid: rustix::process::geteuid().as_raw(),
                pid: std::process::id(),
            },
        )
        .unwrap();
        let path = transport.socket_path().to_path_buf();
        let client = std::thread::spawn(move || {
            let mut stream = UnixStream::connect(path).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let hello = WmV1ClientHello {
                minimum_revision: 3,
                maximum_revision: 3,
                capabilities: if enabled {
                    SOPHIA_WM_CAPABILITY_POINTER_FOCUS
                } else {
                    0
                },
            };
            stream
                .write_all(&encode_wm_v1_client_hello_frame(&hello).unwrap())
                .unwrap();
            let welcome = read_frame(&mut stream);
            assert_eq!(
                decode_wm_v1_server_welcome_frame(&welcome)
                    .unwrap()
                    .capabilities
                    & SOPHIA_WM_CAPABILITY_POINTER_FOCUS
                    != 0,
                enabled
            );
            // Even when hover is refused, the next ordinary request is intact:
            // the guard must reject before writing any prefix to the socket.
            let frame = read_frame(&mut stream);
            let (_, request) = decode_wm_v1_projection_request_frame(&frame).unwrap();
            assert_eq!(request.cause_kind, if enabled { 4 } else { 0 });
        });
        transport
            .accept_and_negotiate(1, Duration::from_secs(2))
            .unwrap();
        let mut request = PolicyProjectionRequest {
            connection_epoch: 1,
            request_id: 1,
            scene_generation: 1,
            policy_generation: 1,
            affected_outputs: vec![OutputId::from_raw(1)],
            cause: PolicyRequestCause::PointerFocus {
                output: OutputId::from_raw(1),
                target: None,
            },
        };
        let result = transport.send_projection_request(TransactionId::from_raw(1), &request);
        if enabled {
            result.unwrap();
        } else {
            assert!(matches!(
                result,
                Err(PolicyTransportError::Transfer(
                    sophia_runtime::PolicyTransferError::UnsupportedCapability
                ))
            ));
            request.cause = PolicyRequestCause::SceneChanged;
            transport
                .send_projection_request(TransactionId::from_raw(2), &request)
                .unwrap();
        }
        client.join().unwrap();
        transport.disconnect().unwrap();
    }
}
