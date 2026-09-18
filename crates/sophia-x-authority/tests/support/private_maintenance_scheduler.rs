#![cfg(all(test, unix))]

use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Exit {
    Stop,
    Error,
    Unwind,
}

pub(super) enum Maintain {
    Step,
    InterruptBudget,
    Finish,
}

#[derive(Debug)]
pub(super) struct ObservedStep {
    pub(super) phase: PrivateMaintenancePhase,
    pub(super) status: PrivateMaintenanceStatus,
    pub(super) charged: bool,
    pub(super) settled: Option<bool>,
    pub(super) allowance: Option<sophia_input_authority::ServiceStartRefusal>,
    pub(super) modifiers: Option<u16>,
    pub(super) native_records: Option<usize>,
}

pub(super) struct MaintainedService {
    pub(super) path: std::path::PathBuf,
    pub(super) registry: XServerFrontendRouteRegistry,
    pub(super) controller: PrivateAuthorityController,
    pub(super) raster: XServerFrontendRasterRouter,
    pub(super) commands: SyncSender<XServerFrontendServiceCommand>,
    pub(super) transactions: Receiver<XAuthorityObservedTransactionBatch>,
    pub(super) acks: Receiver<XAuthorityClientControlAck>,
    pub(super) deliveries: Receiver<XAuthorityClientInputDelivery>,
    pub(super) access: PrivateProducerAccess,
    pub(super) owner: Arc<PrivateServiceOwner>,
    pub(super) maintenance: SyncSender<Maintain>,
    pub(super) steps: Receiver<ObservedStep>,
    pub(super) closed: Receiver<(bool, bool, bool)>,
    done: Receiver<()>,
    thread: Option<std::thread::JoinHandle<()>>,
    telemetry: SeenTelemetry,
}

impl MaintainedService {
    pub(super) fn launch(exit: Exit) -> Self {
        let path = private_service_socket(&format!("maintain-{exit:?}"));
        let config = private_service_config(&path, NamespaceId::from_raw(9871), 4);
        let (commands, service_commands) = sync_channel(4);
        let (transaction_sender, transactions) =
            sync_channel(if matches!(exit, Exit::Unwind) { 1 } else { 64 });
        let (maintain, maintenance) = sync_channel(4);
        let (steps, observed) = channel();
        let (closed, closed_rx) = channel();
        let (done, done_rx) = channel();
        let (ready, prepared) = channel();
        let service_thread = Arc::new(Mutex::new(None));
        let telemetry = Arc::new(Mutex::new(Vec::new()));
        let observer = recording_observer(
            Arc::clone(&telemetry),
            matches!(exit, Exit::Unwind).then_some(XAuthorityBackpressureTelemetryKind::Wait),
            Arc::clone(&service_thread),
        );
        let (parts, acks, deliveries) = producing_parts(4);
        let (port, access) = PrivateProducerAccess::for_service();
        let owner = Arc::new(service_owner(&PrivateSettlementOwner::default(), 4));
        let service_owner = Arc::clone(&owner);
        let thread = std::thread::spawn(move || {
            let owner = service_owner;
            *service_thread.lock().unwrap() = Some(std::thread::current().id());
            let private = PrivateXServerFrontend::new(parts, &owner)
                .unwrap_or_else(|(why, _)| panic!("frontend: {why:?}"));
            let registry = private.broker.registry.clone();
            ready
                .send((
                    registry.clone(),
                    private.broker.raster_router(),
                    private.controller.clone(),
                ))
                .unwrap();
            let mut keeper = PrivateServiceExecutionKeeper::new();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                serve_private_frontend_until_stopped(
                    private,
                    &owner.lease(),
                    &mut keeper,
                    config,
                    transaction_sender,
                    service_commands,
                    port,
                    observer,
                )
            }));
            let unwound = result.is_err();
            let succeeded = matches!(&result, Ok(Ok(_)));
            // This is the real returned settlement (or the unwind's original
            // guard handoff). It enters the independently owned store before
            // any explicit post-collection maintenance call.
            drop(result);
            let collected = keeper.resources.as_ref().is_some_and(|resources| {
                resources
                    .collected
                    .as_ref()
                    .is_some_and(|token| Arc::ptr_eq(&token.registry, &registry.clients))
            });
            assert_eq!(
                keeper.execution().unwrap().availability,
                PrivateExecutionAvailability::Retained
            );
            closed.send((unwound, succeeded, collected)).unwrap();
            while let Ok(command) = maintenance.recv_timeout(Duration::from_secs(5)) {
                match command {
                    Maintain::Finish => break,
                    Maintain::InterruptBudget => {
                        // Labelled fault: abandon one run of the original
                        // retained budget. Neither subsequent phase may reset it.
                        use sophia_input_authority::{CleanupReadiness, ServiceWork};
                        let resources = keeper.resources.as_mut().unwrap();
                        let run = resources
                            .service
                            .start(
                                resources.service_origin.elapsed(),
                                ServiceWork::Cleanup,
                                CleanupReadiness::Eligible,
                            )
                            .unwrap();
                        drop(run);
                    }
                    Maintain::Step => {
                        let step = keeper.maintain_step(&owner.lease());
                        let resources = keeper.resources.as_ref().unwrap();
                        let modifiers = resources.keyboards.modifiers(resources.seat);
                        // Readout only, after the bounded visit and its guards
                        // have finished. This test inventory scan authorizes
                        // no disposal and is not part of the scheduler.
                        let native_records = owner.store.inner.lock().ok().map(|held| {
                            held.terminal
                                .iter()
                                .filter(|inventory| {
                                    inventory.execution.as_ref().is_some_and(|witness| {
                                        Arc::ptr_eq(witness, &resources.lifetime.0)
                                    })
                                })
                                .map(|inventory| inventory.holds.len() + inventory.settling.len())
                                .sum()
                        });
                        steps
                            .send(ObservedStep {
                                phase: step.phase(),
                                status: step.status(),
                                charged: step.charge().is_some_and(Result::is_ok),
                                settled: step.output_settled(),
                                allowance: step.allowance_refusal(),
                                modifiers,
                                native_records,
                            })
                            .unwrap();
                    }
                }
            }
            drop(keeper);
            done.send(()).unwrap();
        });
        let (registry, raster, controller) = prepared.recv_timeout(Duration::from_secs(3)).unwrap();
        Self {
            path,
            registry,
            controller,
            raster,
            commands,
            transactions,
            acks,
            deliveries,
            access,
            owner,
            maintenance: maintain,
            steps: observed,
            closed: closed_rx,
            done: done_rx,
            thread: Some(thread),
            telemetry,
        }
    }

    pub(super) fn step(&self) -> ObservedStep {
        self.maintenance.send(Maintain::Step).unwrap();
        self.steps.recv_timeout(Duration::from_secs(3)).unwrap()
    }

    pub(super) fn finish(mut self) {
        self.maintenance.send(Maintain::Finish).unwrap();
        self.done.recv_timeout(Duration::from_secs(3)).unwrap();
        self.thread.take().unwrap().join().unwrap();
    }
}

impl Drop for MaintainedService {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = self
                .commands
                .try_send(XServerFrontendServiceCommand::StopAndDisconnect);
            let _ = self.maintenance.try_send(Maintain::Finish);
            if self.done.recv_timeout(Duration::from_secs(3)).is_ok() {
                let _ = thread.join();
            }
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn maintained_exit(exit: Exit) {
    let service = MaintainedService::launch(exit);
    let mut client = connect_private_client(&service.path);
    handshake(&mut client);
    let custody = wait_attached(&service.registry);
    let sender = registry_sender(&service.registry, custody.cleanup_record().client);
    match exit {
        Exit::Stop => service
            .commands
            .send(XServerFrontendServiceCommand::StopAndDisconnect)
            .unwrap(),
        Exit::Error => {
            let (acknowledgement, acknowledged) = sync_channel(1);
            drop(acknowledged);
            service
                .commands
                .send(XServerFrontendServiceCommand::UpdateOutputTopology {
                    snapshot: sophia_protocol::OutputTopologySnapshot {
                        generation: 1,
                        primary: sophia_protocol::OutputId::from_raw(1),
                        outputs: Vec::new(),
                    },
                    acknowledgement,
                })
                .unwrap();
        }
        Exit::Unwind => {
            let surface = draw_and_learn_surface(&mut client, &service.transactions);
            assert!(waited_for(|| saw_kind(
                &service.telemetry,
                XAuthorityBackpressureTelemetryKind::Wait,
                true
            )));
            service
                .raster
                .try_route(raster_requirement_for(surface))
                .unwrap();
        }
    }
    let (unwound, succeeded, collected) =
        service.closed.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(unwound, matches!(exit, Exit::Unwind));
    assert_eq!(succeeded, matches!(exit, Exit::Stop));
    assert!(
        collected,
        "the original actual collection token reached this keeper"
    );
    assert!(eof_within(&mut client, 3));
    let unresolved_egress = custody.store().unresolved_egress();
    if matches!(exit, Exit::Unwind) {
        assert_eq!(unresolved_egress, Some(1));
    }
    let first = service.step();
    assert_eq!(first.phase, PrivateMaintenancePhase::Output);
    assert!(first.charged);
    assert_eq!(
        first.modifiers,
        Some(0),
        "the original prepared seat survives"
    );
    assert_eq!(first.native_records, Some(0));
    assert_eq!(
        first.settled,
        Some(false),
        "namespace cleanup did not make the held sender drained"
    );
    assert!(
        custody
            .store()
            .committed_obligation(custody.identity().index)
            .is_some()
    );
    let next = service.step();
    assert_eq!(next.phase, PrivateMaintenancePhase::Terminal);
    assert!(
        next.charged,
        "the second phase uses an accounted visit too: {next:?}"
    );
    drop(sender);
    let mut settled = false;
    for _ in 0..16 {
        let step = service.step();
        if step.settled == Some(true) {
            settled = true;
            break;
        }
        if step.status == PrivateMaintenanceStatus::Yielded {
            std::thread::sleep(Duration::from_millis(17));
        }
    }
    assert!(
        settled,
        "the bounded cursor returned to the now-drained exact home"
    );
    assert!(
        custody
            .store()
            .committed_obligation(custody.identity().index)
            .is_none()
    );
    // The exact socket-ending evidence survives slot/holder return because
    // native terminal dependents may still require this source.
    assert_eq!(
        custody
            .cleanup_record()
            .ordered_home
            .peek_retained(|continuation| continuation.disposition().ending),
        Some(Some(PrivateRetainedEnding::Ended))
    );
    assert_eq!(
        custody.store().unresolved_egress(),
        unresolved_egress,
        "settling one output home does not settle retained service egress"
    );
    std::thread::sleep(Duration::from_millis(17));
    service.maintenance.send(Maintain::InterruptBudget).unwrap();
    for _ in 0..2 {
        let step = service.step();
        assert_eq!(
            step.allowance,
            Some(sophia_input_authority::ServiceStartRefusal::Interrupted)
        );
        assert!(!step.charged);
    }
    service.finish();
}

#[test]
fn maintenance_after_normal_stop_uses_collected_custody_and_surviving_owner() {
    maintained_exit(Exit::Stop);
}

#[test]
fn maintenance_after_service_error_uses_collected_custody_and_surviving_owner() {
    maintained_exit(Exit::Error);
}

#[test]
fn maintenance_after_unwind_uses_collected_custody_and_surviving_owner() {
    maintained_exit(Exit::Unwind);
}

#[test]
fn maintenance_without_resources_and_with_foreign_lease_cannot_visit() {
    let owner = service_owner(&PrivateSettlementOwner::default(), 4);
    let mut keeper = PrivateServiceExecutionKeeper::new();
    assert_eq!(
        keeper.maintain_step(&owner.lease()).status(),
        PrivateMaintenanceStatus::Unavailable
    );
    let (runner, actual, _registration, _channels, _acks, _deliveries) = prepared_runner_fixture();
    drop(keeper.retain(runner, None).shutdown());
    for phase in [
        PrivateMaintenancePhase::Output,
        PrivateMaintenancePhase::Terminal,
    ] {
        let step = keeper.maintain_step(&owner.lease());
        assert_eq!(step.phase(), phase);
        assert_eq!(step.status(), PrivateMaintenanceStatus::Refused);
        assert!(step.charge().is_none());
    }
    drop((keeper, actual));
}
