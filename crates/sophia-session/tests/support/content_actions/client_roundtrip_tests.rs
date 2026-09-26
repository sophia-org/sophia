//! Real private transport + generic client lifecycle/outbox + Session ledger.
//! Presented and policy publication facts are supplied at this boundary. WM
//! admission uses the production borrowed owner/queue. This is not native
//! retirement, policy execution, or compositor owner-loop acceptance.
use super::*;
use sophia_protocol::*;
use sophia_runtime::ShellSessionTransport;
use sophia_shell_client::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Harness {
    transport: ShellSessionTransport,
    client: ShellConnection,
    lifecycle: ContentLifecycle,
    limits: ContentLimits,
    target: PresentedContentTarget,
    ledger: ContentActionLedger,
}

impl Harness {
    fn new() -> Self {
        Self::on_output(5)
    }

    fn on_output(output: u64) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sophia-client-action-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut transport = ShellSessionTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        transport
            .authorize_protected_peer(&sophia_runtime::ProtectionDomainEvidence {
                backend: sophia_runtime::ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [sophia_runtime::ProtectionDomainRole::MetadataShell]
                    .into_iter()
                    .collect(),
            })
            .unwrap();
        let path = transport.socket_path().to_path_buf();
        let connect = std::thread::spawn(move || {
            ShellConnection::connect(
                path,
                ShellClientOptions {
                    minimum_revision: 6,
                    maximum_revision: 6,
                    required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                        | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
                        | SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS
                        | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION,
                    handshake_timeout: Duration::from_secs(2),
                },
            )
            .unwrap()
        });
        transport
            .accept_and_negotiate_with_content_policy(
                1,
                Duration::from_secs(2),
                sophia_runtime::ShellContentAdmissionPolicy::Granted {
                    discrete_input: true,
                },
            )
            .unwrap();
        let mut client = connect.join().unwrap();
        transport.poll_io().unwrap();
        let (_, ShellContentRecord::Limits(limits)) = client.poll_content().unwrap().unwrap()
        else {
            panic!("limits")
        };
        let mut target = super::tests::target();
        target.grant = limits.grant;
        target.output.id = output;
        let mut lifecycle = ContentLifecycle::new(limits.clone()).unwrap();
        lifecycle
            .register(ClientContentCandidate {
                transaction: TransactionId::from_raw(70),
                begin: ContentCandidateBegin {
                    grant: target.grant,
                    output: target.output,
                    candidate_generation: target.candidate_generation,
                    facts_generation: 1,
                    pacing_permit: 1,
                    interaction_generation: target.interaction_generation,
                    surface_count: 1,
                    placement_count: 1,
                    target_count: 1,
                },
                surfaces: vec![ContentSurface {
                    allocation: target.allocation,
                    scale_generation: 1,
                    role: 1,
                    edge: 1,
                    margins: ContentMargins::default(),
                    reservation_extent: 0,
                    parent_surface_index: u16::MAX,
                    anchor_parent_rect: ContentPixelRect::default(),
                }],
                targets: vec![ContentTarget {
                    surface_index: 0,
                    action_kind: 1,
                    target_id: target.target_id,
                    target_generation: target.target_generation,
                    action_id: target.action_id,
                    bounds_px: target.bounds_px,
                }],
            })
            .unwrap();
        Self {
            transport,
            client,
            lifecycle,
            limits,
            target,
            ledger: ContentActionLedger::default(),
        }
    }

    fn outcome(&mut self, kind: u16) {
        self.transport
            .send_async(
                encode_shell_content_frame(
                    TransactionId::from_raw(70),
                    &ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
                        grant: self.target.grant,
                        output: self.target.output,
                        candidate_generation: self.target.candidate_generation,
                        kind,
                        reason: 0,
                        presentation_epoch: if kind == 2 {
                            self.target.presentation_epoch
                        } else {
                            0
                        },
                        work_area_generation: 1,
                        wm_commit_generation: 1,
                    }),
                )
                .unwrap(),
            )
            .unwrap();
    }

    fn dispatch(&mut self) -> ContentDispatch {
        self.transport.poll_io().unwrap();
        let (tx, record) = self
            .client
            .poll_content()
            .unwrap()
            .expect("owned server record");
        self.lifecycle.dispatch(tx, record).unwrap()
    }

    fn issue(&mut self) -> u64 {
        self.ledger
            .issue(
                self.target.clone(),
                0,
                &self.limits,
                TransactionId::from_raw(71),
                &mut self.transport.connection(),
            )
            .unwrap()
            .unwrap()
    }
}

fn ack(action: &ContentAction) -> ContentActionAck {
    ContentActionAck {
        grant: action.grant,
        output: action.output,
        candidate_generation: action.candidate_generation,
        presentation_epoch: action.presentation_epoch,
        interaction_generation: action.interaction_generation,
        allocation: action.allocation,
        target_id: action.target_id,
        target_generation: action.target_generation,
        action_id: action.action_id,
        event_id: action.event_id,
        disposition: ACK_CONSUMED,
    }
}

#[test]
fn real_client_roundtrip_keeps_receipt_and_activation_independent() {
    let mut h = Harness::on_output(1);
    let grant = h.limits.grant;
    let mut queue = std::collections::VecDeque::new();
    let mut next_transaction = 100;
    let mut admissions = [0; 2];
    for round in 0..1000 {
        let (output, origin, ack_first) = [
            (1, 0, false),
            (2, 2560, true),
            (1, 0, true),
            (2, -1920, false),
        ][round % 4];
        if round != 0 {
            let mut candidate = h
                .lifecycle
                .presented(h.target.output)
                .unwrap()
                .candidate
                .clone();
            h.target.output.id = output;
            h.target.allocation.id = 4 + output;
            h.target.candidate_generation += 1;
            h.target.presentation_epoch += 1;
            candidate.begin.output = h.target.output;
            candidate.begin.candidate_generation = h.target.candidate_generation;
            candidate.surfaces[0].allocation = h.target.allocation;
            h.lifecycle.register(candidate).unwrap();
        }
        assert_eq!(h.target.grant, grant);
        let before = h
            .lifecycle
            .presented(h.target.output)
            .map(|p| p.candidate.begin.candidate_generation);
        h.outcome(1);
        h.dispatch();
        assert_eq!(
            h.lifecycle
                .presented(h.target.output)
                .map(|p| p.candidate.begin.candidate_generation),
            before,
            "Prepared must retain the previous targets"
        );
        h.outcome(2);
        // Presented geometry is supplied by this fixture, as before. Drive the
        // actual global-to-output capture before entering the socket/WM chain.
        let mut binding = sophia_engine::PresentedContentBinding {
            grant: h.target.grant,
            output: h.target.output,
            candidate_generation: h.target.candidate_generation,
            presentation_epoch: h.target.presentation_epoch,
            interaction_generation: h.target.interaction_generation,
            transform: sophia_engine::PresentedContentTransform {
                viewport: Rect {
                    x: origin,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                layout_generation: 1,
            },
            authority_current: true,
            targets: vec![h.target.clone()],
            popouts: Vec::new(),
            allocations: vec![(
                h.target.allocation,
                h.target.allocation_logical,
                h.target.allocation_pixel,
            )],
        };
        let mut capture = sophia_engine::ContentCaptureState::default();
        for pressed in [true, false] {
            let disposition = sophia_engine::resolve_content_pointer_event(
                &mut capture,
                SeatId::from_raw(1),
                DeviceId::from_raw(1),
                InputEventKind::PointerButton {
                    button: 0x110,
                    pressed,
                },
                Some(Point {
                    x: f64::from(origin) + 4.0,
                    y: 4.0,
                }),
                Some(&binding),
                false,
            );
            assert_eq!(
                disposition,
                if pressed {
                    sophia_engine::ContentPointerDisposition::Captured
                } else {
                    sophia_engine::ContentPointerDisposition::Activated(h.target.clone())
                }
            );
            if pressed {
                // Process the pressed candidate, then present a new raster
                // before release. Only the shared presentation reducer can
                // preserve button continuity; native completion is supplied.
                h.dispatch();
                let mut candidate = h
                    .lifecycle
                    .presented(h.target.output)
                    .unwrap()
                    .candidate
                    .clone();
                h.target.candidate_generation += 1;
                h.target.presentation_epoch += 1;
                candidate.begin.candidate_generation = h.target.candidate_generation;
                h.lifecycle.register(candidate).unwrap();
                h.outcome(1);
                h.dispatch();
                h.outcome(2);
                let mut next = binding.clone();
                next.candidate_generation = h.target.candidate_generation;
                next.presentation_epoch = h.target.presentation_epoch;
                next.targets[0] = h.target.clone();
                sophia_engine::reconcile_content_continuity(Some(&binding), &mut next);
                h.target = next.targets[0].clone();
                binding = next;
            }
        }
        let event = h.issue(); // Both records traverse the same actual server FIFO.
        let presented = h.dispatch();
        assert_eq!(presented.transaction.raw(), 70);
        assert!(matches!(presented.record, ShellContentRecord::CandidateOutcome(v) if v.kind == 2));
        let dispatched = h.dispatch();
        assert_eq!(dispatched.action, Some(ContentActionDispatch::Eligible));
        let ShellContentRecord::Action(action) = dispatched.record else {
            panic!("action")
        };
        let mut refreshed = binding.clone();
        refreshed.candidate_generation += 1;
        refreshed.presentation_epoch += 1;
        refreshed.targets[0].candidate_generation += 1;
        refreshed.targets[0].presentation_epoch += 1;
        sophia_engine::reconcile_content_continuity(Some(&binding), &mut refreshed);
        assert_eq!(h.ledger.next_cancellation(&[refreshed.clone()], 1), None);
        // The exact issued record remains unchanged; no reissue or new effect.
        assert_eq!(h.ledger.live[0].action, action);
        let activation = ShellIndicatorActivation {
            connection_epoch: action.grant.connection_epoch,
            snapshot_generation: 7,
            output: OutputId::from_raw(action.output.id),
            indicator: action.target_id,
            action: action.action_id,
            event_id: event,
        };
        h.client
            .enqueue_indicator_action_response(
                TransactionId::from_raw(80),
                &ack(&action),
                Some((TransactionId::from_raw(81), &activation)),
            )
            .unwrap();
        h.client.poll_io().unwrap();
        if ack_first {
            assert_eq!(
                h.ledger
                    .service_acks(&mut h.transport.connection(), 1, 64)
                    .unwrap(),
                1
            );
        }
        let tx = TransactionId::from_raw(81);
        assert_eq!(
            h.ledger.live[0].ack,
            if ack_first {
                AckState::Consumed
            } else {
                AckState::Awaiting
            }
        );
        assert_eq!(
            h.ledger.indicator_admission(&activation, 1),
            LinkedIndicatorAdmission::Eligible
        );
        use crate::live_session::LiveIndicatorAdmission;
        let publication = publication(&activation);
        let outputs = [sophia_engine::HeadlessOutput {
            id: activation.output,
            size: Size {
                width: 100,
                height: 100,
            },
            scale: 1,
        }];
        let expected_serial = next_transaction;
        let snapshot = crate::shell_indicator_projection::indicator_snapshot(
            &publication,
            Some(activation.output),
            activation.connection_epoch,
        );
        let mut indicators =
            crate::live_session::metadata_shell::indicators::LiveIndicatorState::default();
        indicators.last_published = Some(snapshot);
        h.ledger
            .service_indicator_request(
                &mut h.transport.connection(),
                &mut indicators,
                true,
                1,
                |action, output| {
                    let result = LiveIndicatorAdmission {
                        policy_connection_epoch: 1,
                        publication: &publication,
                        outputs: &outputs,
                        output_generations: &outputs.iter().map(|o| (o.id, 1)).collect(),
                        capabilities: sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS,
                        next_transaction: &mut next_transaction,
                        queue: &mut queue,
                        in_flight_source: None,
                        in_flight: false,
                    }
                    .enqueue(action, output)?;
                    assert_eq!(result.policy_connection_epoch, 1);
                    assert_eq!(result.activation_serial, Some(expected_serial));
                    assert!(matches!(queue[0].cause, PolicyRequestCause::OutputAction { activation_serial, .. } if Some(activation_serial) == result.activation_serial));
                    Ok(result)
                },
            )
            .unwrap();
        assert_eq!(queue.len(), 1);
        assert!(
            matches!(queue[0].cause, PolicyRequestCause::OutputAction { activation_serial, action: v, output, output_generation: 1 } if activation_serial == expected_serial && v.raw() == activation.action && output == activation.output)
        );
        assert_eq!(queue[0].affected_outputs, vec![activation.output]);
        assert_eq!(
            h.ledger.indicator_admission(&activation, 1),
            LinkedIndicatorAdmission::Stale
        );
        assert_eq!(
            h.ledger
                .service_acks(&mut h.transport.connection(), 1, 64)
                .unwrap(),
            usize::from(!ack_first)
        );
        assert!(h.ledger.live.is_empty());
        h.transport.poll_io().unwrap();
        let (outcome_tx, outcome) = h
            .client
            .poll_indicator_activation_outcome()
            .unwrap()
            .unwrap();
        assert_eq!(outcome_tx, tx);
        assert_eq!(
            (outcome.event_id, outcome.status),
            (event, ShellIndicatorActivationStatus::Accepted)
        );

        assert!(
            h.transport
                .poll_kind(IpcMessageKind::ShellIndicatorActivate)
                .unwrap()
                .is_none()
        );
        admissions[output as usize - 1] += 1;
        queue.pop_front().unwrap();
        h.lifecycle.finish_action(event);
    }
    assert_eq!(admissions, [500, 500]);
    assert_eq!(h.ledger.issued_high_water, 1000);
    assert!(queue.is_empty());
    h.transport.disconnect().unwrap();
}

#[test]
fn real_client_rejects_action_before_presented_and_never_acknowledges_cancel() {
    let mut h = Harness::new();
    h.outcome(1);
    h.dispatch();
    let event = h.issue(); // Intentionally invalid publisher ordering control.
    let dispatched = h.dispatch();
    assert_eq!(dispatched.action, Some(ContentActionDispatch::Rejected));
    let now = u64::from(h.limits.action_ack_timeout_ms) + 1;
    let index = h.ledger.next_cancellation(&[], now).unwrap();
    h.ledger
        .queue_cancellation(
            index,
            TransactionId::from_raw(72),
            &mut h.transport.connection(),
        )
        .unwrap();
    let cancel = h.dispatch();
    assert_eq!(cancel.action, Some(ContentActionDispatch::Cancelled));
    assert!(
        matches!(cancel.record, ShellContentRecord::Action(v) if v.event_id == event && v.kind == ACTION_CANCEL)
    );
    h.client.poll_io().unwrap();
    assert!(h.transport.poll_content_action_ack().unwrap().is_none());
    assert!(
        h.transport
            .poll_kind(IpcMessageKind::ShellIndicatorActivate)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        h.ledger
            .service_acks(&mut h.transport.connection(), now, 64)
            .unwrap(),
        0
    );
    assert!(h.ledger.live.is_empty());
    h.transport.disconnect().unwrap();
}

fn publication(activation: &ShellIndicatorActivation) -> sophia_engine::PolicyIndicatorPublication {
    sophia_engine::PolicyIndicatorPublication {
        generation: activation.snapshot_generation,
        connection_epoch: Some(1),
        tab_groups: Vec::new(),
        output_statuses: Vec::new(),
        indicators: vec![PolicyProjectionIndicator {
            output: activation.output,
            slot: 0,
            indicator: activation.indicator,
            action: Some(WmActionId::from_raw(activation.action)),
            state_bits: 0,
            label: "workspace".to_owned(),
        }],
    }
}

#[test]
fn shared_wm_admission_keeps_unpublished_and_capacity_refusals_out_of_the_queue() {
    use crate::live_session::{
        LiveIndicatorAdmission, LiveWmRequestAdmission, WM_OWNER_REQUEST_CAPACITY,
    };
    let activation = ShellIndicatorActivation {
        connection_epoch: 1,
        snapshot_generation: 2,
        output: OutputId::from_raw(5),
        indicator: 11,
        action: 13,
        event_id: 7,
    };
    let published = publication(&activation);
    let outputs = [5, 8].map(|id| sophia_engine::HeadlessOutput {
        id: OutputId::from_raw(id),
        size: Size {
            width: 100,
            height: 100,
        },
        scale: 1,
    });
    let mut queue = std::collections::VecDeque::new();
    let mut next_transaction = 100;
    let mut owner = LiveIndicatorAdmission {
        policy_connection_epoch: 1,
        publication: &published,
        outputs: &outputs,
        output_generations: &outputs.iter().map(|o| (o.id, 1)).collect(),
        capabilities: sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS,
        next_transaction: &mut next_transaction,
        queue: &mut queue,
        in_flight_source: None,
        in_flight: false,
    };
    owner.capabilities = 0;
    assert_eq!(
        owner
            .enqueue(WmActionId::from_raw(13), activation.output)
            .unwrap()
            .admission,
        LiveWmRequestAdmission::Duplicate
    );
    assert_eq!(*owner.next_transaction, 100);
    assert!(owner.queue.is_empty());
    owner.capabilities = sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS;
    assert_eq!(
        owner
            .enqueue(WmActionId::from_raw(14), activation.output)
            .unwrap()
            .admission,
        LiveWmRequestAdmission::Duplicate
    );
    assert_eq!(
        owner
            .enqueue(WmActionId::from_raw(13), outputs[1].id)
            .unwrap()
            .admission,
        LiveWmRequestAdmission::Duplicate
    );
    assert!(owner.queue.is_empty());
    assert_eq!(*owner.next_transaction, 100);
    for _ in 0..WM_OWNER_REQUEST_CAPACITY {
        assert_eq!(
            owner
                .enqueue(WmActionId::from_raw(13), activation.output)
                .unwrap()
                .admission,
            LiveWmRequestAdmission::Admitted
        );
    }
    let before = owner
        .queue
        .iter()
        .map(|cause| cause.cause)
        .collect::<Vec<_>>();
    assert_eq!(
        owner
            .enqueue(WmActionId::from_raw(13), activation.output)
            .unwrap()
            .admission,
        LiveWmRequestAdmission::RejectedCapacity
    );
    assert_eq!(
        owner
            .queue
            .iter()
            .map(|cause| cause.cause)
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(
        owner.queue[0].affected_outputs,
        vec![outputs[0].id, outputs[1].id]
    );
    // Existing in-flight work consumes the same finite capacity as queued work.
    owner.queue.pop_front();
    owner.in_flight = true;
    assert_eq!(
        owner
            .enqueue(WmActionId::from_raw(13), activation.output)
            .unwrap()
            .admission,
        LiveWmRequestAdmission::RejectedCapacity
    );
    owner.in_flight = false;
    assert_eq!(
        owner
            .enqueue(WmActionId::from_raw(13), activation.output)
            .unwrap()
            .admission,
        LiveWmRequestAdmission::Admitted
    );
}

#[test]
fn owner_decision_finishes_refusals_without_replaying_wm_admission() {
    use crate::live_session::LiveWmRequestAdmission as Admission;
    use crate::live_session::metadata_shell::indicators::LiveIndicatorState;
    use crate::shell_indicator_projection::indicator_snapshot;
    use ShellIndicatorActivationStatus as Status;
    for (initial, linked, admission, expected, calls) in [
        (
            Status::Unknown,
            true,
            Admission::Admitted,
            Status::Unknown,
            0,
        ),
        (
            Status::Accepted,
            false,
            Admission::Admitted,
            Status::Stale,
            0,
        ),
        (
            Status::Accepted,
            true,
            Admission::RejectedCapacity,
            Status::Unknown,
            1,
        ),
        (
            Status::Accepted,
            true,
            Admission::Duplicate,
            Status::Stale,
            1,
        ),
        (
            Status::Accepted,
            true,
            Admission::Admitted,
            Status::Accepted,
            1,
        ),
    ] {
        let mut h = Harness::new();
        h.outcome(2);
        h.dispatch();
        let event = h.issue();
        let ShellContentRecord::Action(action) = h.dispatch().record else {
            panic!("action")
        };
        let activation = ShellIndicatorActivation {
            connection_epoch: action.grant.connection_epoch,
            snapshot_generation: action.target_generation,
            output: OutputId::from_raw(action.output.id),
            indicator: action.target_id,
            action: action.action_id,
            event_id: event,
        };
        h.client
            .enqueue_indicator_action_response(
                TransactionId::from_raw(80),
                &ack(&action),
                Some((TransactionId::from_raw(81), &activation)),
            )
            .unwrap();
        h.client.poll_io().unwrap();
        let transaction = TransactionId::from_raw(81);
        let mut indicators = LiveIndicatorState::default();
        let mut snapshot = indicator_snapshot(
            &publication(&activation),
            Some(activation.output),
            activation.connection_epoch,
        );
        if initial == Status::Unknown {
            snapshot.indicators.clear();
        }
        indicators.last_published = Some(snapshot);
        if !linked {
            h.ledger.wm_rejected(event, 1);
        }
        let mut invoked = 0;
        h.ledger
            .service_indicator_request(
                &mut h.transport.connection(),
                &mut indicators,
                true,
                1,
                |_, _| {
                    invoked += 1;
                    Ok(crate::live_session::LiveIndicatorAdmissionResult {
                        admission,
                        activation_serial: Some(100),
                        policy_connection_epoch: 1,
                    })
                },
            )
            .unwrap();
        assert_eq!(invoked, calls);
        h.transport.poll_io().unwrap();
        let (tx, outcome) = h
            .client
            .poll_indicator_activation_outcome()
            .unwrap()
            .unwrap();
        assert_eq!(
            (tx, outcome.event_id, outcome.status),
            (transaction, event, expected)
        );
        assert!(h.transport.poll_indicator_activation().unwrap().is_none());
        assert_eq!(invoked, calls);
        if expected == Status::Accepted {
            assert_eq!(h.ledger.live[0].activation, ActivationState::EffectAdmitted);
        }
    }
}

#[test]
fn direct_mode_keeps_snapshot_and_event_high_water_checks() {
    use crate::live_session::metadata_shell::indicators::LiveIndicatorState;
    use crate::shell_indicator_projection::indicator_snapshot;
    let mut h = Harness::new();
    let mut indicators = LiveIndicatorState::default();
    let mut calls = 0;
    for (index, event, fresh, expected) in [
        (0, 1, true, ShellIndicatorActivationStatus::Accepted),
        (1, 1, true, ShellIndicatorActivationStatus::Stale),
        (2, 2, false, ShellIndicatorActivationStatus::Stale),
        (3, 2, true, ShellIndicatorActivationStatus::Accepted),
    ] {
        let activation = ShellIndicatorActivation {
            connection_epoch: h.target.grant.connection_epoch,
            snapshot_generation: h.target.target_generation,
            output: OutputId::from_raw(h.target.output.id),
            indicator: h.target.target_id,
            action: h.target.action_id,
            event_id: event,
        };
        let mut snapshot = indicator_snapshot(
            &publication(&activation),
            Some(activation.output),
            activation.connection_epoch,
        );
        if !fresh {
            snapshot.generation += 1;
        }
        indicators.last_published = Some(snapshot);
        // Direct mode has no content ledger entry. The paired generic sender
        // is only the request encoder here; its ACK is not serviced as input.
        let action = action_from_target(&h.target, event, ACTION_ACTIVATE);
        let transaction = TransactionId::from_raw(101 + index * 2);
        h.client
            .enqueue_indicator_action_response(
                TransactionId::from_raw(100 + index * 2),
                &ack(&action),
                Some((transaction, &activation)),
            )
            .unwrap();
        h.client.poll_io().unwrap();
        let before = calls;
        assert!(
            h.ledger
                .service_indicator_request(
                    &mut h.transport.connection(),
                    &mut indicators,
                    false,
                    1,
                    |_, _| {
                        calls += 1;
                        Ok(crate::live_session::LiveIndicatorAdmissionResult {
                            admission: crate::live_session::LiveWmRequestAdmission::Admitted,
                            activation_serial: Some(100),
                            policy_connection_epoch: 1,
                        })
                    },
                )
                .unwrap()
        );
        assert_eq!(
            calls - before,
            usize::from(expected == ShellIndicatorActivationStatus::Accepted)
        );
        h.transport.poll_io().unwrap();
        let (tx, outcome) = h
            .client
            .poll_indicator_activation_outcome()
            .unwrap()
            .unwrap();
        assert_eq!((tx, outcome.status), (transaction, expected));
        assert!(
            !h.ledger
                .service_indicator_request(
                    &mut h.transport.connection(),
                    &mut indicators,
                    false,
                    1,
                    |_, _| panic!("empty intake must not replay admission"),
                )
                .unwrap()
        );
    }
    assert_eq!(calls, 2);
    assert!(h.ledger.live.is_empty());
}
