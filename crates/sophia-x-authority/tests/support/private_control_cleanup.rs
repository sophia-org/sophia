use super::private_maintenance_scheduler::{Exit, MaintainedService};
use super::*;

fn interrupted_configure() -> (
    MaintainedService,
    Arc<PrivateControlClientSource>,
    ControlCompletionToken,
    Arc<PrivateEvidenceCustody>,
) {
    interrupted_configure_with_stop(false)
}

fn interrupted_configure_with_stop(
    stop_before_client_end: bool,
) -> (
    MaintainedService,
    Arc<PrivateControlClientSource>,
    ControlCompletionToken,
    Arc<PrivateEvidenceCustody>,
) {
    let service = MaintainedService::launch(Exit::Stop);
    service.access.await_ready(Duration::from_secs(15)).unwrap();
    let mut client = connect_private_client(&service.path);
    let window = handshake_ids(&mut client) | 0x0d01;
    let (surface, _) = selecting_window(&mut client, &service.transactions, window, 0);
    let custody = wait_attached(&service.registry);
    let source = custody
        .cleanup_record()
        .connection_state
        .get()
        .unwrap()
        .control_source
        .get()
        .unwrap()
        .upgrade()
        .unwrap();
    let original = source
        .endpoint
        .registration
        .get()
        .unwrap()
        .selections
        .lock()
        .unwrap()
        .geometry(XResourceId::new(u64::from(window), 1))
        .unwrap();
    source.fail_after_runtime.store(true, Ordering::Release);
    let owner = service.owner.clone();
    service
        .access
        .control_producer(&owner.lease())
        .unwrap()
        .submit(
            &owner.lease(),
            configure(custody.cleanup_record().client, surface, 99881),
        )
        .unwrap();
    let completion = service.registry.control_completion().unwrap();
    let cleanup = waited_for_value(|| completion.cleanups_owed().unwrap().into_iter().next())
        .expect("actual writer exit abandons the interrupted control");
    assert_eq!(cleanup.steps.runtime, ControlStepState::Completed);
    assert_eq!(cleanup.steps.projection, ControlStepState::NotStarted);
    assert_eq!(
        source
            .state
            .runtime
            .lock()
            .unwrap()
            .window_geometry(
                source.endpoint.namespace,
                XResourceId::new(u64::from(window), 1)
            )
            .unwrap(),
        Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 60
        }
    );
    assert_eq!(
        source
            .endpoint
            .registration
            .get()
            .unwrap()
            .selections
            .lock()
            .unwrap()
            .geometry(XResourceId::new(u64::from(window), 1)),
        Some(original)
    );
    assert!(
        service.acks.try_recv().is_err(),
        "no invented acknowledgement for an interrupted effect"
    );
    if stop_before_client_end {
        service
            .commands
            .send(XServerFrontendServiceCommand::StopAndDisconnect)
            .unwrap();
        assert!(
            service
                .closed
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .2
        );
        drop(client);
        return (service, source, cleanup.token, custody);
    }
    drop(client);
    assert!(
        waited_for(|| source.teardown.lock().unwrap().finished),
        "actual resource cleanup and its publication finish while service remains live"
    );
    assert!(
        service
            .transactions
            .try_iter()
            .any(|batch| batch.removed_surfaces.contains(&surface)),
        "the real teardown publication names the original surface"
    );
    let _ = service
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect);
    assert!(
        service
            .closed
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .2
    );
    (service, source, cleanup.token, custody)
}

fn drive_control(service: &MaintainedService, turns: usize) {
    for _ in 0..turns {
        let _ = super::final_custody_step(service);
    }
}

#[test]
fn actual_partial_configure_cleanup_uses_original_removal_and_returns_its_credit() {
    let (service, source, token, custody) = interrupted_configure();
    let completion = service.registry.control_completion().unwrap();
    let before = service.owner.store.reserved().unwrap();
    assert!(before > 0);
    drive_control(&service, 800);
    assert_eq!(completion.state_of(token), ControlRecordState::Retired);
    assert!(service.owner.store.reserved().unwrap() < before);
    assert!(
        source
            .endpoint
            .registration
            .get()
            .unwrap()
            .selections
            .lock()
            .unwrap()
            .geometries
            .is_empty()
    );
    assert!(service.acks.try_recv().is_err());
    drive_control(&service, 50);
    assert_eq!(service.owner.store.reserved(), Some(0));
    drop(custody);
    service.finish();
}

#[test]
fn partial_configure_withheld_source_removal_keeps_original_record_and_credit() {
    let (service, source, token, _custody) = interrupted_configure();
    let actual = source.teardown.lock().unwrap().removed.take().unwrap();
    drive_control(&service, 400);
    assert_eq!(
        service
            .registry
            .control_completion()
            .unwrap()
            .state_of(token),
        ControlRecordState::Outstanding
    );
    assert!(service.owner.store.reserved().unwrap() > 0);
    assert_eq!(service.owner.custodies_kept(), 1);
    source.teardown.lock().unwrap().removed = Some(actual);
    drive_control(&service, 800);
    assert_eq!(
        service
            .registry
            .control_completion()
            .unwrap()
            .state_of(token),
        ControlRecordState::Retired
    );
    service.finish();
}

#[test]
fn partial_configure_foreign_source_cannot_retire_an_identically_numbered_operation() {
    let (first, original, token, _custody) = interrupted_configure();
    let (foreign, other, _, _foreign_custody) = interrupted_configure();
    assert_eq!(original.endpoint.client, other.endpoint.client);
    assert_eq!(original.endpoint.namespace, other.endpoint.namespace);
    let completion = first.registry.control_completion().unwrap();
    let execution = completion
        .inner
        .lock()
        .unwrap()
        .records
        .iter()
        .find(|record| record.token == token)
        .unwrap()
        .source
        .clone()
        .unwrap();
    execution.lock().unwrap().source = other;
    drive_control(&first, 400);
    assert_eq!(completion.state_of(token), ControlRecordState::Outstanding);
    execution.lock().unwrap().source = original;
    drive_control(&first, 800);
    assert_eq!(completion.state_of(token), ControlRecordState::Retired);
    first.finish();
    foreign.finish();
}

#[test]
fn partial_configure_missing_teardown_publication_keeps_native_removal_separate() {
    let (service, source, token, _custody) = interrupted_configure();
    source.teardown.lock().unwrap().finished = false;
    drive_control(&service, 400);
    assert_eq!(
        service
            .registry
            .control_completion()
            .unwrap()
            .state_of(token),
        ControlRecordState::Outstanding
    );
    assert!(source.teardown.lock().unwrap().removed.is_some());
    source.teardown.lock().unwrap().finished = true;
    drive_control(&service, 800);
    assert_eq!(
        service
            .registry
            .control_completion()
            .unwrap()
            .state_of(token),
        ControlRecordState::Retired
    );
    service.finish();
}

#[test]
fn partial_configure_replacement_removal_receipt_cannot_answer_original_source() {
    let (service, source, token, _custody) = interrupted_configure();
    let actual_registration = {
        let mut teardown = source.teardown.lock().unwrap();
        let endpoint = &mut teardown.removed.as_mut().unwrap().endpoint;
        // Labelled receipt fault: preserve every number, replace only its
        // registration identity. No native source or receipt is fabricated.
        std::mem::replace(
            &mut endpoint.registration,
            Arc::new(std::sync::OnceLock::new()),
        )
    };
    drive_control(&service, 400);
    assert_eq!(
        service
            .registry
            .control_completion()
            .unwrap()
            .state_of(token),
        ControlRecordState::Outstanding
    );
    source
        .teardown
        .lock()
        .unwrap()
        .removed
        .as_mut()
        .unwrap()
        .endpoint
        .registration = actual_registration;
    drive_control(&service, 800);
    assert_eq!(
        service
            .registry
            .control_completion()
            .unwrap()
            .state_of(token),
        ControlRecordState::Retired
    );
    service.finish();
}

#[test]
fn partial_configure_stop_keeps_actual_cancelled_teardown_publication_payload() {
    let (service, source, token, _custody) = interrupted_configure_with_stop(true);
    {
        let teardown = source.teardown.lock().unwrap();
        let removed = teardown
            .removed
            .as_ref()
            .expect("actual native removal occurred");
        let pending = teardown
            .pending_publication
            .as_ref()
            .expect("the original cancelled batch remains owned");
        assert_eq!(pending.removed_surfaces, removed.resources.removed_surfaces);
        assert!(!pending.removed_surfaces.is_empty());
        assert!(teardown.properties_removed && teardown.publication_started && !teardown.finished);
    }
    drive_control(&service, 500);
    assert_eq!(
        service
            .registry
            .control_completion()
            .unwrap()
            .state_of(token),
        ControlRecordState::Outstanding
    );
    assert_eq!(service.owner.custodies_kept(), 1);
    assert!(
        source
            .teardown
            .lock()
            .unwrap()
            .pending_publication
            .is_some()
    );
    assert!(service.acks.try_recv().is_err());
    service.finish();
}
