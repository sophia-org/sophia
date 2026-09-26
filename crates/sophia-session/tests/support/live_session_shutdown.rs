#![cfg(test)]

use super::super::shutdown::{
    AuthorityWorkWait, disconnect_frontend_for_drain, take_authority_work,
};
use super::super::{
    AuthorityIngressState, SessionQuiescence, SessionQuiescenceDecision, SessionQuiescenceSnapshot,
    XAuthorityObservedTransactionBatch, XServerFrontendServiceCommand,
    drain_queued_authority_batches, observe_authority_ingress, stop_frontend_intake,
};
use sophia_protocol::TransactionId;
use std::collections::VecDeque;
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};

#[test]
fn disconnected_ingress_still_consumes_final_removal_and_coordinator() {
    let (sender, receiver) = sync_channel(2);
    sender.send(batch(1)).unwrap();
    let mut removal = batch(2);
    removal
        .removed_surfaces
        .push(sophia_protocol::SurfaceId::new(7, 1));
    removal
        .released_dma_bufs
        .push(sophia_protocol::BufferHandle::from_raw(8));
    removal
        .released_fences
        .push(sophia_protocol::FenceHandle::from_raw(9));
    sender.send(removal).unwrap();
    drop(sender);
    let mut initial = Some(receiver.recv().unwrap());
    let mut queued = VecDeque::new();
    let ingress = drain_queued_authority_batches(
        &receiver,
        &mut queued,
        2,
        Duration::from_millis(20),
        AuthorityIngressState::Open,
    );
    assert_eq!(ingress, AuthorityIngressState::Disconnected);
    for expected in [1, 2, 3] {
        let selected = take_authority_work(
            &mut initial,
            &mut queued,
            Some(TransactionId::from_raw(3)),
            ingress,
            false,
        )
        .unwrap();
        assert_eq!(selected.transaction, TransactionId::from_raw(expected));
        if expected == 2 {
            assert_eq!(selected.removed_surfaces.len(), 1);
            assert_eq!(
                selected.released_dma_bufs,
                [sophia_protocol::BufferHandle::from_raw(8)]
            );
            assert_eq!(
                selected.released_fences,
                [sophia_protocol::FenceHandle::from_raw(9)]
            );
            assert_eq!(
                super::super::authority_merge_run_len(&selected, queued.iter(), true, 8,),
                1
            );
        }
    }
    assert!(initial.is_none());
    assert!(queued.is_empty());
    assert_eq!(
        take_authority_work(&mut initial, &mut queued, None, ingress, false,).unwrap_err(),
        AuthorityWorkWait::Service
    );
}

#[test]
fn owner_service_defers_but_never_consumes_buffered_work() {
    for ingress in [
        AuthorityIngressState::Open,
        AuthorityIngressState::Disconnected,
    ] {
        let mut initial = Some(batch(1));
        let mut queued = VecDeque::from([batch(2)]);
        for _ in 0..3 {
            assert_eq!(
                take_authority_work(&mut initial, &mut queued, None, ingress, true,).unwrap_err(),
                AuthorityWorkWait::Service
            );
            assert_eq!(
                initial.as_ref().unwrap().transaction,
                TransactionId::from_raw(1)
            );
            assert_eq!(queued.len(), 1);
        }
        for expected in [1, 2] {
            assert_eq!(
                take_authority_work(&mut initial, &mut queued, None, ingress, false,)
                    .unwrap()
                    .transaction,
                TransactionId::from_raw(expected)
            );
        }
        assert_eq!(
            take_authority_work(&mut initial, &mut queued, None, ingress, false,).unwrap_err(),
            match ingress {
                AuthorityIngressState::Open => AuthorityWorkWait::Receive,
                AuthorityIngressState::Disconnected => AuthorityWorkWait::Service,
            }
        );
    }
}

#[test]
fn capacity_boundaries_preserve_fifo_through_final_disconnect() {
    for capacity in [1, 2, 4] {
        let (sender, receiver) = sync_channel(5);
        for transaction in 1..=5 {
            sender.send(batch(transaction)).unwrap();
        }
        drop(sender);
        let mut queued = VecDeque::new();
        let mut initial = None;
        let mut ingress = AuthorityIngressState::Open;
        for expected in 1..=5 {
            ingress = drain_queued_authority_batches(
                &receiver,
                &mut queued,
                capacity,
                Duration::from_secs(1),
                ingress,
            );
            assert!(queued.len() <= capacity);
            assert_eq!(
                take_authority_work(&mut initial, &mut queued, None, ingress, false,)
                    .unwrap()
                    .transaction,
                TransactionId::from_raw(expected)
            );
        }
        ingress = drain_queued_authority_batches(
            &receiver,
            &mut queued,
            capacity,
            Duration::from_secs(1),
            ingress,
        );
        assert_eq!(ingress, AuthorityIngressState::Disconnected);
        assert!(queued.is_empty());
    }
}

#[test]
fn known_closed_ingress_is_not_polled_again() {
    let (sender, receiver) = sync_channel(1);
    sender.send(batch(1)).unwrap();
    let mut queued = VecDeque::new();
    assert_eq!(
        drain_queued_authority_batches(
            &receiver,
            &mut queued,
            2,
            Duration::from_secs(1),
            AuthorityIngressState::Disconnected,
        ),
        AuthorityIngressState::Disconnected
    );
    assert!(queued.is_empty());
    assert_eq!(
        receiver.try_recv().unwrap().transaction,
        TransactionId::from_raw(1)
    );
}

fn batch(transaction: u64) -> XAuthorityObservedTransactionBatch {
    super::super::wm_update_coordinator_batch(TransactionId::from_raw(transaction))
}

#[test]
fn opportunistic_drain_preserves_final_batch_before_disconnect() {
    let (sender, receiver) = sync_channel(2);
    sender.send(batch(1)).unwrap();
    sender.send(batch(2)).unwrap();
    drop(sender);
    assert_eq!(
        receiver.recv().unwrap().transaction,
        TransactionId::from_raw(1)
    );

    let mut queued = VecDeque::new();
    assert_eq!(
        drain_queued_authority_batches(
            &receiver,
            &mut queued,
            2,
            Duration::from_millis(20),
            AuthorityIngressState::Open
        ),
        AuthorityIngressState::Disconnected
    );
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].transaction, TransactionId::from_raw(2));
}

#[test]
fn quiescence_accepts_disconnect_only_after_buffered_work_settles() {
    let now = Instant::now();
    let mut quiescence = Some(SessionQuiescence::new(
        "test",
        now,
        Duration::from_millis(20),
    ));

    observe_authority_ingress(
        AuthorityIngressState::Disconnected,
        &mut quiescence,
        now + Duration::from_millis(1),
    )
    .unwrap();
    let quiescence = quiescence.unwrap();
    assert_eq!(
        quiescence.decision(
            now + Duration::from_millis(2),
            SessionQuiescenceSnapshot {
                pending_authority_batches: 1,
                ..SessionQuiescenceSnapshot::default()
            },
        ),
        SessionQuiescenceDecision::Pending
    );
    assert_eq!(
        quiescence.decision(
            now + Duration::from_millis(3),
            SessionQuiescenceSnapshot::default(),
        ),
        SessionQuiescenceDecision::Complete
    );
}

#[test]
fn authority_disconnect_before_quiescence_remains_fatal() {
    let mut quiescence = None;
    assert_eq!(
        observe_authority_ingress(
            AuthorityIngressState::Disconnected,
            &mut quiescence,
            Instant::now(),
        ),
        Err("persistent X authority transaction channel disconnected")
    );
}

#[test]
fn frontend_stop_is_idempotent_after_successful_stop() {
    let (sender, receiver) = sync_channel(1);
    let mut stopped = false;
    stop_frontend_intake(&sender, &mut stopped).unwrap();
    assert!(matches!(
        receiver.recv().unwrap(),
        XServerFrontendServiceCommand::StopAccepting
    ));
    drop(receiver);

    stop_frontend_intake(&sender, &mut stopped).unwrap();
    assert!(stopped);
}

#[test]
fn frontend_drain_disconnect_is_idempotent_after_successful_request() {
    let (sender, receiver) = sync_channel(1);
    let mut stopped = false;
    disconnect_frontend_for_drain(&sender, &mut stopped).unwrap();
    assert!(matches!(
        receiver.recv().unwrap(),
        XServerFrontendServiceCommand::DrainAndDisconnect
    ));
    drop(receiver);

    disconnect_frontend_for_drain(&sender, &mut stopped).unwrap();
    assert!(stopped);
}

#[test]
fn initial_frontend_stop_failure_is_retained() {
    let (sender, receiver) = sync_channel(1);
    drop(receiver);
    let mut stopped = false;

    assert!(stop_frontend_intake(&sender, &mut stopped).is_err());
    assert!(!stopped);
}

#[test]
fn recovery_cannot_extend_the_deadline_or_bypass_an_existing_drain() {
    use super::super::shutdown::native_recovery_allowed;
    let now = Instant::now();
    let deadline = now + Duration::from_millis(10);
    // Rejected VT, disable timeout, resume and topology all share this guard.
    assert!(native_recovery_allowed(Some(deadline), now, false, false));
    assert!(!native_recovery_allowed(
        Some(deadline),
        deadline,
        false,
        false
    ));
    assert!(!native_recovery_allowed(None, now, true, false));
    assert!(!native_recovery_allowed(None, now, false, true));
    assert!(native_recovery_allowed(None, now, false, false));
}

#[cfg(feature = "native-session")]
#[test]
fn component_outer_cleanup_retains_owner_until_native_disposition_and_prior_error_resolve() {
    use super::super::component_lifecycle;
    use super::super::metadata_shell::component_session::ShellComponentSession;
    use sophia_config::{ShellComponentConfig, ShellComponentRole, ShellGpuMode};
    use sophia_runtime::ShellContentAdmissionPolicy;
    let root = std::env::temp_dir().join(format!("outer-component-cleanup-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut owner = Some(
        ShellComponentSession::prepare(
            &[ShellComponentConfig {
                id: "menu".into(),
                role: ShellComponentRole::ApplicationLauncher,
                executable: "/nonexistent-sophia-native-launcher".into(),
                config: None,
                reservation: None,
                gpu: ShellGpuMode::Denied,
                transport: Default::default(),
            }],
            28,
            None,
            &root,
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        )
        .unwrap(),
    );
    let components = owner.as_mut().unwrap();
    components.set_presentation_available(true).unwrap();
    assert!(components.start(0).is_err());
    let key = components.attempt(0).unwrap();
    let outputs = [sophia_engine::HeadlessOutput {
        id: sophia_protocol::OutputId::from_raw(1),
        size: sophia_protocol::Size {
            width: 64,
            height: 64,
        },
        scale: 1,
    }];
    let mut runtime = super::super::LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut failures = Vec::new();
    component_lifecycle::stop(owner.as_mut(), Some(&mut runtime), &mut failures);
    assert!(failures.is_empty());
    assert!(!owner.as_ref().unwrap().process_retained(key));
    assert_eq!(owner.as_ref().unwrap().pending_revocations(), 0);
    let mut directory = Some(root.clone());
    component_lifecycle::finish(&mut owner, &mut directory, false, true, &mut failures);
    assert!(owner.is_some());
    assert!(root.join("menu").is_dir());
    assert_eq!(directory, Some(root.clone()));
    assert_eq!(failures.len(), 1);
    failures.clear();
    component_lifecycle::finish(&mut owner, &mut directory, true, false, &mut failures);
    assert!(
        owner.is_some(),
        "earlier loop error retains final owner in error carrier"
    );
    assert!(root.join("menu").is_dir());
    // Supplied native completion flags are fixture inputs, not hardware evidence.
    component_lifecycle::finish(&mut owner, &mut directory, true, true, &mut failures);
    assert!(failures.is_empty());
    assert!(owner.is_none());
    assert!(directory.is_none());
    assert!(!root.exists());
}

#[cfg(feature = "native-session")]
#[test]
fn component_endpoint_survives_inside_actual_terminal_error_carrier() {
    use super::super::metadata_shell::component_session::ShellComponentSession;
    use super::super::native_owner_retirement::{NativeRetirement, RetirementFailure};
    let root = std::env::temp_dir().join(format!("component-error-carrier-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let owner = ShellComponentSession::prepare(
        &[sophia_config::ShellComponentConfig {
            id: "menu".into(),
            role: sophia_config::ShellComponentRole::ApplicationLauncher,
            executable: "/bin/true".into(),
            config: None,
            reservation: None,
            gpu: sophia_config::ShellGpuMode::Denied,
            transport: Default::default(),
        }],
        28,
        None,
        &root,
        sophia_runtime::ShellContentAdmissionPolicy::Denied,
    )
    .unwrap();
    let retirement: NativeRetirement = NativeRetirement::default();
    let error = RetirementFailure::new(
        "earlier loop error".into(),
        retirement,
        (Some(owner), root.clone()),
    );
    assert!(root.join("menu").is_dir());
    let returned: Box<dyn std::error::Error> = Box::new(error);
    assert_eq!(returned.to_string(), "earlier loop error");
    assert!(root.join("menu").is_dir());
    drop(returned);
    assert!(!root.join("menu").exists());
    std::fs::remove_dir(root).unwrap();
}
