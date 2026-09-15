//! Real private transport + generic client lifecycle/outbox + Session ledger.
//! Presented and policy publication facts are supplied at this boundary. WM
//! admission uses the production borrowed owner/queue. This is not native
//! retirement, policy execution, or compositor owner-loop acceptance.
use super::*;
use sophia_protocol::*;
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
                &mut self.transport,
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
    for ack_first in [false, true] {
        let mut h = Harness::new();
        h.outcome(1);
        h.dispatch();
        assert!(h.lifecycle.presented(h.target.output).is_none());
        h.outcome(2);
        let event = h.issue(); // Both records traverse the same actual server FIFO.
        let presented = h.dispatch();
        assert_eq!(presented.transaction.raw(), 70);
        assert!(matches!(presented.record, ShellContentRecord::CandidateOutcome(v) if v.kind == 2));
        let dispatched = h.dispatch();
        assert_eq!(dispatched.action, Some(ContentActionDispatch::Eligible));
        let ShellContentRecord::Action(action) = dispatched.record else {
            panic!("action")
        };
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
            assert_eq!(h.ledger.service_acks(&mut h.transport, 1, 64).unwrap(), 1);
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
        let mut queue = std::collections::VecDeque::new();
        let mut next_transaction = 100;
        let snapshot = crate::live_session::metadata_shell::indicators::indicator_snapshot(
            &publication,
            Some(activation.output),
            activation.connection_epoch,
        );
        let mut indicators =
            crate::live_session::metadata_shell::indicators::LiveIndicatorState::default();
        indicators.last_published = Some(snapshot);
        h.ledger
            .service_indicator_request(
                &mut h.transport,
                &mut indicators,
                true,
                1,
                |action, output| {
                    LiveIndicatorAdmission {
                        publication: &publication,
                        outputs: &outputs,
                        active_output: activation.output,
                        next_transaction: &mut next_transaction,
                        queue: &mut queue,
                        in_flight_source: None,
                        in_flight: false,
                    }
                    .enqueue(action, output)
                },
            )
            .unwrap();
        assert_eq!(queue.len(), 1);
        assert!(
            matches!(queue[0].cause, PolicyRequestCause::Action { activation_serial: 100, action: v } if v.raw() == activation.action)
        );
        assert_eq!(queue[0].affected_outputs, vec![activation.output]);
        assert_eq!(
            h.ledger.indicator_admission(&activation, 1),
            LinkedIndicatorAdmission::Stale
        );
        assert_eq!(
            h.ledger.service_acks(&mut h.transport, 1, 64).unwrap(),
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
        h.transport.disconnect().unwrap();
    }
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
        .queue_cancellation(index, TransactionId::from_raw(72), &mut h.transport)
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
    assert_eq!(h.ledger.service_acks(&mut h.transport, now, 64).unwrap(), 0);
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
        publication: &published,
        outputs: &outputs,
        active_output: outputs[1].id,
        next_transaction: &mut next_transaction,
        queue: &mut queue,
        in_flight_source: None,
        in_flight: false,
    };
    assert_eq!(
        owner
            .enqueue(WmActionId::from_raw(14), activation.output)
            .unwrap(),
        LiveWmRequestAdmission::Duplicate
    );
    assert_eq!(
        owner
            .enqueue(WmActionId::from_raw(13), outputs[1].id)
            .unwrap(),
        LiveWmRequestAdmission::Duplicate
    );
    assert!(owner.queue.is_empty());
    assert_eq!(*owner.next_transaction, 100);
    for _ in 0..WM_OWNER_REQUEST_CAPACITY {
        assert_eq!(
            owner
                .enqueue(WmActionId::from_raw(13), activation.output)
                .unwrap(),
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
            .unwrap(),
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
        vec![outputs[1].id, outputs[0].id]
    );
    // Existing in-flight work consumes the same finite capacity as queued work.
    owner.queue.pop_front();
    owner.in_flight = true;
    assert_eq!(
        owner
            .enqueue(WmActionId::from_raw(13), activation.output)
            .unwrap(),
        LiveWmRequestAdmission::RejectedCapacity
    );
    owner.in_flight = false;
    assert_eq!(
        owner
            .enqueue(WmActionId::from_raw(13), activation.output)
            .unwrap(),
        LiveWmRequestAdmission::Admitted
    );
}

#[test]
fn owner_decision_finishes_refusals_without_replaying_wm_admission() {
    use crate::live_session::LiveWmRequestAdmission as Admission;
    use crate::live_session::metadata_shell::indicators::{LiveIndicatorState, indicator_snapshot};
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
            .service_indicator_request(&mut h.transport, &mut indicators, true, 1, |_, _| {
                invoked += 1;
                Ok(admission)
            })
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
            assert_eq!(h.ledger.live[0].activation, ActivationState::WmAdmitted);
        }
    }
}

#[test]
fn direct_mode_keeps_snapshot_and_event_high_water_checks() {
    use crate::live_session::metadata_shell::indicators::{LiveIndicatorState, indicator_snapshot};
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
                .service_indicator_request(&mut h.transport, &mut indicators, false, 1, |_, _| {
                    calls += 1;
                    Ok(crate::live_session::LiveWmRequestAdmission::Admitted)
                },)
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
                    &mut h.transport,
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
