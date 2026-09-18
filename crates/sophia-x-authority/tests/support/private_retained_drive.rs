#![cfg(all(test, unix))]

use super::*;

// Component controls use a real registration, worker, join, commitment and
// original runner resources. Only the collection token is staged: this
// fixture has no service collection frame. These are not M3 acceptance cases.
struct DriveFixture {
    keeper: PrivateServiceOwner,
    execution: PrivateServiceExecutionKeeper,
    custody: Arc<PrivateEvidenceCustody>,
    registry: XServerFrontendRouteRegistry,
    cursor: PrivateRetainedDriveCursor,
    sender: Option<PrivateGatedOrderedSender>,
    _peer: UnixStream,
}

impl DriveFixture {
    fn prepared(client: u64) -> Self {
        let f = worker_fixture(XServerFrontendClientId(client));
        f.permit();
        let pin = custody_for(&f, &f.fixture.keeper);
        let custody = Arc::clone(&pin.custody);
        let registry = worker_registry(&f.fixture.runner).clone();
        start_worker(&f, &custody, None);
        drop(pin);
        drop(f.fixture.registration);
        let workers = stop_and_collect(&f.fixture.keeper.lease(), &registry, &custody);
        assert_eq!(workers.len(), 1);
        assert!(workers[0].joined);
        let collected = collected_for(&registry);
        let cleanup = custody.visit_deferred_cleanup(Some(&collected));
        let cleanup = cleanup.result.unwrap();
        assert!(cleanup.committed);
        assert_eq!(cleanup.namespace, PrivateNamespaceClearance::Established);
        let next = f
            .fixture
            .keeper
            .inventory
            .kept
            .lock()
            .unwrap()
            .places
            .iter()
            .position(|entry| {
                entry
                    .as_ref()
                    .is_some_and(|entry| Arc::ptr_eq(entry, &custody))
            })
            .unwrap();
        let mut runner = f.fixture.runner;
        runner.close_admission();
        let mut execution = PrivateServiceExecutionKeeper::new();
        drop(execution.retain(runner, Some(collected)).shutdown());
        Self {
            keeper: f.fixture.keeper,
            execution,
            custody,
            registry,
            cursor: PrivateRetainedDriveCursor { next },
            sender: Some(f.sender),
            _peer: f.peer,
        }
    }

    fn resources(&mut self) -> &mut PrivateRetainedExecutionResources {
        let instance = self.execution.execution().unwrap().instance;
        self.execution
            .resources_for(&self.registry, instance)
            .unwrap()
    }

    fn step(&mut self) -> PrivateRetainedDriveStep {
        let resources = self.execution.resources.as_mut().unwrap();
        resources.drive_retained_output_step(&self.keeper.lease(), &mut self.cursor)
    }

    fn pin(&self) -> PrivateCustodyPin<'_> {
        self.keeper.custody_named(self.custody.identity()).unwrap()
    }

    fn authorize(&self) -> Result<PrivateRetainedDriveAuthority<'_>, PrivateRetainedDriveRefusal> {
        PrivateRetainedDriveAuthority::authorize(
            self.pin(),
            &self.registry,
            self.execution
                .resources
                .as_ref()
                .unwrap()
                .collected
                .as_ref(),
        )
    }
}

fn charged_outcome(
    step: PrivateRetainedDriveStep,
) -> Result<PrivateRetainedVisit, PrivateRetainedDriveRefusal> {
    match step {
        PrivateRetainedDriveStep::Charged {
            outcome,
            charge,
            supervision,
        } => {
            assert!(charge.is_ok(), "{charge:?}");
            assert!(supervision.is_ok(), "{supervision:?}");
            outcome
        }
        other => panic!("expected one charged visit: {other:?}"),
    }
}

#[test]
fn retained_drive_uses_one_original_cleanup_start_and_exact_settled_disposal() {
    let mut f = DriveFixture::prepared(9601);
    let index = f.custody.identity().index;
    assert!(f.custody.store().committed_obligation(index).is_some());
    // Namespace clearance already succeeded, yet the sender keeps the
    // source's drained predicate false and the place must remain owned.
    assert_eq!(
        charged_outcome(f.step()),
        Ok(PrivateRetainedVisit::Driven { settled: false })
    );
    assert!(f.custody.store().committed_obligation(index).is_some());
    assert_eq!(f.resources().service.usage().cleanup_starts, 1);
    let before = f.custody.store().continuations_reserved();
    drop(f.sender.take());
    let authority = f
        .authorize()
        .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    assert_eq!(
        authority.visit(),
        Ok(PrivateRetainedVisit::Driven { settled: true })
    );
    assert_eq!(
        f.custody.store().continuations_reserved(),
        before.map(|count| count - 1)
    );
    assert!(f.custody.store().committed_obligation(index).is_none());
    assert!(matches!(
        f.authorize(),
        Err(PrivateRetainedDriveRefusal::Stale)
    ));
}

#[test]
fn retained_drive_refuses_foreign_lease_and_collection_without_touching_home() {
    let mut f = DriveFixture::prepared(9602);
    let foreign = service_owner(&PrivateSettlementOwner::default(), 4);
    let before = f.resources().service.usage();
    let mut cursor = PrivateRetainedDriveCursor::default();
    assert!(matches!(
        f.resources()
            .drive_retained_output_step(&foreign.lease(), &mut cursor),
        PrivateRetainedDriveStep::Refused(PrivateRetainedDriveRefusal::ForeignServiceOwner)
    ));
    assert_eq!(f.resources().service.usage(), before);
    let other = DriveFixture::prepared(9603);
    let token = collected_for(&other.registry);
    assert!(matches!(
        PrivateRetainedDriveAuthority::authorize(f.pin(), &f.registry, Some(&token)),
        Err(PrivateRetainedDriveRefusal::Prerequisite(
            PrivateDeferredCleanupRefusal::ConnectionsUncollected
        ))
    ));
    f.resources().collected = None;
    assert_eq!(
        charged_outcome(f.step()),
        Err(PrivateRetainedDriveRefusal::Prerequisite(
            PrivateDeferredCleanupRefusal::ConnectionsUncollected
        ))
    );
}

#[test]
fn retained_drive_requires_actual_commitment_and_exact_published_join() {
    let f = DriveFixture::prepared(9604);
    let other = DriveFixture::prepared(9605);
    let index = f.custody.identity().index;
    let original = {
        let mut store = f.custody.store().inner.lock().unwrap();
        let PrivateHolderPlace::Taken(holder) = &mut store.holders[index] else {
            panic!("committed")
        };
        holder.obligation.take().unwrap()
    };
    assert!(matches!(
        f.authorize(),
        Err(PrivateRetainedDriveRefusal::Uncommitted)
    ));
    {
        let mut store = f.custody.store().inner.lock().unwrap();
        let PrivateHolderPlace::Taken(holder) = &mut store.holders[index] else {
            panic!("committed")
        };
        holder.obligation = Some(PrivateCommittedObligation {
            identity: original.identity.clone(),
            closed: original.closed,
            join: Arc::downgrade(other.custody.join()),
        });
    }
    assert!(matches!(
        f.authorize(),
        Err(PrivateRetainedDriveRefusal::ForeignCommitment)
    ));
    {
        let mut store = f.custody.store().inner.lock().unwrap();
        let PrivateHolderPlace::Taken(holder) = &mut store.holders[index] else {
            panic!("committed")
        };
        holder.obligation = Some(original);
    }
    assert!(f.authorize().is_ok());
    {
        let mut store = f.custody.store().inner.lock().unwrap();
        let PrivateHolderPlace::Taken(holder) = &mut store.holders[index] else {
            panic!("committed")
        };
        holder.credit.armed = false;
    }
    assert!(matches!(
        f.authorize(),
        Err(PrivateRetainedDriveRefusal::ForeignCommitment)
    ));
    {
        let mut store = f.custody.store().inner.lock().unwrap();
        let PrivateHolderPlace::Taken(holder) = &mut store.holders[index] else {
            panic!("committed")
        };
        holder.credit.armed = true;
    }
}

#[test]
fn retained_drive_requires_join_publication_then_recorded_fence_before_permission() {
    let f = worker_fixture(XServerFrontendClientId(9614));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner).clone();
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let token = collected_for(&registry);
    let pin = || f.fixture.keeper.custody_named(custody.identity()).unwrap();
    assert!(matches!(
        PrivateRetainedDriveAuthority::authorize(pin(), &registry, Some(&token)),
        Err(PrivateRetainedDriveRefusal::Prerequisite(
            PrivateDeferredCleanupRefusal::JoinUnpublished
        ))
    ));
    stop_and_collect(&f.fixture.keeper.lease(), &registry, &custody);
    assert!(matches!(
        PrivateRetainedDriveAuthority::authorize(pin(), &registry, Some(&token)),
        Err(PrivateRetainedDriveRefusal::FenceUnestablished)
    ));
    assert!(
        custody
            .store()
            .committed_obligation(custody.identity().index)
            .is_none()
    );
}

#[test]
fn retained_drive_refuses_an_unreadable_fence_even_when_duty_is_committed() {
    let f = worker_fixture(XServerFrontendClientId(9615));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner).clone();
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    stop_and_collect(&f.fixture.keeper.lease(), &registry, &custody);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = custody.gate().fenced.lock().unwrap();
        panic!("stage an interrupted producer handover");
    }));
    let token = collected_for(&registry);
    assert_eq!(
        custody.visit_deferred_cleanup(Some(&token)).result,
        Err(PrivateDeferredCleanupRefusal::ClosureUnestablished)
    );
    assert!(
        custody
            .store()
            .committed_obligation(custody.identity().index)
            .is_some()
    );
    assert!(matches!(
        PrivateRetainedDriveAuthority::authorize(custody, &registry, Some(&token)),
        Err(PrivateRetainedDriveRefusal::FenceUnestablished)
    ));
}

#[test]
fn retained_drive_refuses_live_poisoned_and_mismatched_home_evidence() {
    let f = DriveFixture::prepared(9606);
    let home = &f.custody.cleanup_record().ordered_home;
    home.state.lock().unwrap().standing = PrivateHomeStanding::Live;
    assert_eq!(
        f.authorize().ok().unwrap().visit(),
        Err(PrivateRetainedDriveRefusal::HomeLive)
    );
    home.state.lock().unwrap().standing = PrivateHomeStanding::Retained;
    home.borrow(|continuation| match continuation {
        PrivateOrderedContinuation::Setup { evidence, .. }
        | PrivateOrderedContinuation::Serving { evidence, .. } => evidence.fence = None,
    });
    assert_eq!(
        f.authorize().ok().unwrap().visit(),
        Err(PrivateRetainedDriveRefusal::HomeEvidenceMismatch)
    );
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = home.state.lock().unwrap();
        panic!("stage an interrupted home mutation");
    }));
    assert_eq!(
        f.authorize().ok().unwrap().visit(),
        Err(PrivateRetainedDriveRefusal::HomeUnreadable)
    );
}

#[test]
fn retained_drive_refuses_interrupted_cleanup_even_after_commitment() {
    let f = DriveFixture::prepared(9607);
    f.custody
        .set_deferred_cleanup(PrivateDeferredCleanupStanding::Claimed {
            progress: PrivateDeferredCleanupProgress {
                committed: true,
                ..Default::default()
            },
        });
    assert!(matches!(
        f.authorize(),
        Err(PrivateRetainedDriveRefusal::CleanupInterrupted)
    ));
}

#[test]
fn retained_drive_visits_only_one_physical_place_and_advances_past_refusal() {
    let mut f = DriveFixture::prepared(9608);
    let first = f.cursor.next;
    let places = f.keeper.inventory.kept.lock().unwrap().places.len();
    f.resources().collected = None;
    assert!(charged_outcome(f.step()).is_err());
    assert_eq!(f.cursor.next, (first + 1) % places);
    let before = f.resources().service.usage().cleanup_starts;
    let _ = charged_outcome(f.step());
    assert_eq!(f.cursor.next, (first + 2) % places);
    assert_eq!(f.resources().service.usage().cleanup_starts, before + 1);
}

#[test]
fn retained_drive_cannot_refresh_interrupted_budget_or_failed_watchdog() {
    use sophia_input_authority::{CleanupReadiness, ServiceStartRefusal, ServiceWork};
    let mut f = DriveFixture::prepared(9609);
    let before = f.cursor.next;
    let resources = f.resources();
    drop(
        resources
            .service
            .start(
                resources.service_origin.elapsed(),
                ServiceWork::Cleanup,
                CleanupReadiness::Eligible,
            )
            .unwrap(),
    );
    assert!(matches!(
        f.step(),
        PrivateRetainedDriveStep::Yield(ServiceStartRefusal::Interrupted)
    ));
    assert_eq!(f.cursor.next, before);
    let mut f = DriveFixture::prepared(9610);
    let before = f.cursor.next;
    drop(
        f.resources()
            .watch
            .as_ref()
            .unwrap()
            .begin_dequeued(std::time::Instant::now())
            .unwrap(),
    );
    assert!(matches!(
        f.step(),
        PrivateRetainedDriveStep::Charged {
            outcome: Err(PrivateRetainedDriveRefusal::Supervisor(
                private_watchdog::PrivateWatchdogRefusal::Failed(_)
            )),
            charge: Ok(_),
            supervision: Err(_),
        }
    ));
    assert_eq!(f.cursor.next, before);
}

#[test]
fn retained_drive_releases_store_and_inventory_before_waiting_for_home() {
    let mut f = DriveFixture::prepared(9611);
    let home = Arc::clone(&f.custody.cleanup_record().ordered_home);
    let store = f.custody.store().clone();
    let inventory = Arc::clone(&f.keeper.inventory);
    let (locked_tx, locked_rx) = sync_channel(1);
    let blocker = std::thread::spawn(move || {
        let guard = home.state.lock().unwrap();
        locked_tx.send(()).unwrap();
        // Keep the home unavailable past the independent watch's deadline.
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            store.inner.try_lock().is_ok(),
            "driver held store while blocked on home"
        );
        assert!(
            inventory.kept.try_lock().is_ok(),
            "driver held inventory while blocked on home"
        );
        drop(guard);
    });
    locked_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let step = f.step();
    blocker.join().unwrap();
    assert!(
        matches!(step, PrivateRetainedDriveStep::Charged {
        charge: Ok(sophia_input_authority::ServiceCharge { elapsed, .. }),
        supervision: Err(private_watchdog::PrivateWatchdogRefusal::Failed(_)), ..
    } if elapsed >= Duration::from_millis(250)),
        "{step:?}"
    );
}

// Fault seam: a source-minted capsule is readdressed to this fixture's exact
// retained endpoint, then its writer-owned send-report point is interrupted.
// No claim is made that the synthetic prefix went to a real peer.
fn stage_frame(
    f: &DriveFixture,
    progress: impl FnOnce(usize) -> X11OrderedSendProgress,
) -> (Arc<PrivateDeliveryCompletion>, usize) {
    f.custody
        .cleanup_record()
        .ordered_home
        .borrow(|continuation| {
            let PrivateOrderedContinuation::Serving { owner, .. } = continuation else {
                panic!("serving")
            };
            assert!(owner.in_flight().is_none());
            let endpoint = owner.served.endpoint().clone();
            let (emission, _) =
                private_native_tests::emission_and_endpoint_for_writer_fixture(9690);
            let emission = emission.readdressed(
                f.custody.cleanup_record().client,
                endpoint.generation,
                endpoint,
            );
            let mut capsule = XAuthorityOrderedDelivery::from_emission(emission)
                .unwrap_or_else(|_| panic!("delivery-bearing emission"));
            let id = capsule.delivery();
            let (recovery, _receipts) = claim_fixture(id);
            let completion = recovery.completion_for(id).unwrap().unwrap();
            capsule.carry_finalizer(Arc::new(finalizer_from_held(
                &recovery,
                &completion,
                id,
                capsule.client(),
            )));
            let bytes = capsule
                .emission()
                .encode_frame(0, XByteOrder::LittleEndian, 7)
                .unwrap();
            let len = bytes.as_bytes().len();
            owner.in_flight = Some(X11OrderedInFlight {
                delivery: capsule,
                frame: 0,
                send: X11OrderedSendState {
                    frame: Some(X11OrderedFrame {
                        bytes,
                        progress: progress(len),
                    }),
                    blocked: Duration::ZERO,
                },
            });
            // The worker may already have established termination during its
            // return. Remove that evidence only for this unsupported-state seam.
            owner.closing = None;
            owner.ending_established = false;
            (completion, len)
        })
        .unwrap()
}

#[test]
fn retained_drive_preserves_partial_unknown_and_complete_unretired_frames_distinctly() {
    for (offset, expected) in [
        (0, "incomplete"),
        (5, "incomplete"),
        (usize::MAX, "unknown"),
        (usize::MAX - 1, "complete"),
    ] {
        let f = DriveFixture::prepared(9612);
        let (completion, len) = stage_frame(&f, |len| match expected {
            "unknown" => X11OrderedSendProgress::Unknown { from: 5 },
            "complete" => X11OrderedSendProgress::Sent(len),
            _ => X11OrderedSendProgress::Sent(offset),
        });
        let home = &f.custody.cleanup_record().ordered_home;
        let before = home
            .borrow(|continuation| {
                let PrivateOrderedContinuation::Serving { owner, .. } = continuation else {
                    panic!("serving")
                };
                let held = owner.in_flight().unwrap();
                let frame = held.send.frame.as_ref().unwrap();
                (
                    held.delivery().delivery(),
                    held.frame_index(),
                    frame.progress,
                    frame.bytes.as_ref().to_vec(),
                    Arc::as_ptr(&owner.served.endpoint().registration),
                )
            })
            .unwrap();
        let refused = match expected {
            "unknown" => PrivateRetainedFrame::Indeterminate { from: 5, len },
            "complete" => PrivateRetainedFrame::CompleteUnretired { len },
            _ => PrivateRetainedFrame::Incomplete { sent: offset, len },
        };
        assert_eq!(
            f.authorize().ok().unwrap().visit(),
            Err(PrivateRetainedDriveRefusal::FramePreserved(refused))
        );
        let after = home
            .borrow(|continuation| {
                let PrivateOrderedContinuation::Serving { owner, .. } = continuation else {
                    panic!("serving")
                };
                assert!(
                    owner.closing().is_none(),
                    "no new termination was inferred or attempted"
                );
                let held = owner.in_flight().unwrap();
                let frame = held.send.frame.as_ref().unwrap();
                (
                    held.delivery().delivery(),
                    held.frame_index(),
                    frame.progress,
                    frame.bytes.as_ref().to_vec(),
                    Arc::as_ptr(&owner.served.endpoint().registration),
                )
            })
            .unwrap();
        assert_eq!(
            after, before,
            "capsule, exact frame bytes and cursor, origin remain owned"
        );
        assert!(completion.answer().is_none());
    }
}

#[test]
fn retained_drive_adjudicates_unknown_only_after_its_exact_source_establishes_termination() {
    let f = DriveFixture::prepared(9613);
    let (completion, _) = stage_frame(&f, |_| X11OrderedSendProgress::Unknown { from: 5 });
    f.custody
        .cleanup_record()
        .ordered_home
        .borrow(|continuation| {
            let PrivateOrderedContinuation::Serving { owner, .. } = continuation else {
                panic!("serving")
            };
            owner
                .begin_close(X11OrderedCloseCause::SupervisorStopped)
                .expect("the exact socket ended");
            assert_eq!(
                owner.closing().unwrap().termination,
                X11OrderedTermination::Established
            );
        });
    assert_eq!(
        f.authorize().ok().unwrap().visit(),
        Ok(PrivateRetainedVisit::Driven { settled: false })
    );
    assert_eq!(
        completion.answer().map(|answer| answer.outcome),
        Some(XAuthorityInputDeliveryOutcome::ClientDisconnected)
    );
}
