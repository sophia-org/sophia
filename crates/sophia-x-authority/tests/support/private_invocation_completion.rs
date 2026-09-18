use private_maintenance_scheduler::Maintain;

fn closed_custody_service(
    owner: Arc<PrivateServiceOwner>,
    keep_settlement: bool,
) -> (MaintainedService, Arc<PrivateEvidenceCustody>) {
    let service = MaintainedService::launch_owned(Exit::Stop, owner, keep_settlement);
    service.access.await_ready(Duration::from_secs(15)).unwrap();
    let mut client = connect_private_client(&service.path);
    handshake(&mut client);
    let custody = wait_attached(&service.registry);
    service
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    assert_eq!(
        service.closed.recv_timeout(Duration::from_secs(5)).unwrap(),
        (false, true, true)
    );
    assert!(eof_within(&mut client, 3));
    (service, custody)
}

fn final_custody_step(service: &MaintainedService) -> private_maintenance_scheduler::ObservedStep {
    let step = service.step();
    if step.status == PrivateMaintenanceStatus::Yielded {
        std::thread::sleep(Duration::from_millis(17));
    }
    assert!(
        !matches!(
            step.status,
            PrivateMaintenanceStatus::AccountingFailed
                | PrivateMaintenanceStatus::SupervisionFailed
        ),
        "{step:?}"
    );
    step
}

fn wait_final_custody(service: &MaintainedService) {
    let mut completed = false;
    for _ in 0..800 {
        completed |= final_custody_step(service).completed;
        if service.owner.custodies_kept() == 0 {
            break;
        }
    }
    assert!(
        completed,
        "the original invocation produced positive completion"
    );
    assert_eq!(service.owner.custodies_kept(), 0);
}

#[test]
fn final_custody_waits_for_actual_settlement_handoff_and_original_storage_return() {
    let owner = Arc::new(service_owner(&PrivateSettlementOwner::with_capacity(16), 4));
    let (service, custody) = closed_custody_service(owner, true);
    let home = &custody.cleanup_record().ordered_home;
    for _ in 0..300 {
        let step = final_custody_step(&service);
        assert!(!step.handed_off && !step.completed);
        assert_eq!(
            service.owner.custodies_kept(),
            1,
            "no missing store row may stand for the caller's actual returned settlement"
        );
    }
    assert!(home.storage_returned.load(Ordering::Acquire));
    // Withhold the actual source-produced storage-return fact after it
    // occurred, without changing any source payload or claiming a replacement.
    let returned = home.storage_returned.swap(false, Ordering::AcqRel);
    service
        .maintenance
        .send(Maintain::ReleaseSettlement)
        .unwrap();
    for _ in 0..400 {
        let step = final_custody_step(&service);
        assert!(step.handed_off);
        assert!(!step.completed);
        assert_eq!(
            service.owner.custodies_kept(),
            1,
            "empty current store slot is not the original storage-return receipt"
        );
    }
    home.storage_returned.store(returned, Ordering::Release);
    wait_final_custody(&service);
    let weak = Arc::downgrade(&custody);
    drop(custody);
    assert!(
        weak.upgrade().is_none(),
        "no global owner cycle retains the disposed custody"
    );
    service.finish();
}

#[test]
fn final_custody_reuses_original_slots_across_more_invocations_than_capacity() {
    let owner = Arc::new(service_owner(&PrivateSettlementOwner::with_capacity(16), 4));
    assert_eq!(owner.inventory.kept.lock().unwrap().places.len(), 4);
    for _ in 0..7 {
        let (service, custody) = closed_custody_service(owner.clone(), false);
        assert_eq!(owner.custodies_kept(), 1);
        wait_final_custody(&service);
        let held = owner.store.inner.lock().unwrap();
        assert_eq!(
            (held.continuation_slots, held.holders_taken),
            (0, 0),
            "original output slot, internal holder and destination were returned"
        );
        drop(held);
        drop(custody);
        service.finish();
    }
}

#[test]
fn completion_scan_restarts_for_a_same_invocation_obligation_moved_behind_its_cursor() {
    let owner = Arc::new(service_owner(&PrivateSettlementOwner::with_capacity(16), 4));
    let (service, custody) = closed_custody_service(owner.clone(), false);
    let mut late_scan = false;
    for _ in 0..600 {
        let step = final_custody_step(&service);
        if step.completion_class == 9 && !step.completed {
            late_scan = true;
            break;
        }
    }
    assert!(
        late_scan,
        "the actual charged scan passed the earlier store classes"
    );
    // Labelled store fault: this inert retained control tests class/provenance
    // scanning, not actual control execution or an integrated acceptance case.
    owner.store.reserve().unwrap();
    owner.store.take_indeterminate(
        &service.registry,
        PrivateOperation::Control(
            configure(
                custody.cleanup_record().client,
                SurfaceId::new(9911, 1),
                99110,
            ),
            None,
        ),
    );
    for _ in 0..400 {
        let step = final_custody_step(&service);
        assert!(step.handed_off && !step.completed);
    }
    assert_eq!(owner.custodies_kept(), 1);
    assert_eq!(owner.store.indeterminate(), Some(1));
    service.finish();
}

#[test]
fn final_custody_preserves_foreign_custody_and_indeterminate_rows_in_the_same_store() {
    let owner = Arc::new(service_owner(&PrivateSettlementOwner::with_capacity(16), 4));
    let (first, first_custody) = closed_custody_service(owner.clone(), false);
    let (second, second_custody) = closed_custody_service(owner.clone(), true);
    // Labelled inert store fault, deliberately retaining a foreign registry
    // with colliding client/namespace numbers. It must remain independently owed.
    owner.store.reserve().unwrap();
    owner.store.take_indeterminate(
        &second.registry,
        PrivateOperation::Control(
            configure(
                second_custody.cleanup_record().client,
                SurfaceId::new(9911, 1),
                99110,
            ),
            None,
        ),
    );
    for _ in 0..800 {
        let step = final_custody_step(&first);
        if step.completed && owner.custodies_kept() == 1 {
            break;
        }
    }
    assert_eq!(owner.custodies_kept(), 1);
    let kept = owner.inventory.kept.lock().unwrap();
    assert!(
        kept.places
            .iter()
            .flatten()
            .any(|custody| Arc::ptr_eq(custody, &second_custody))
    );
    assert!(
        !kept
            .places
            .iter()
            .flatten()
            .any(|custody| Arc::ptr_eq(custody, &first_custody))
    );
    drop(kept);
    assert_eq!(owner.store.indeterminate(), Some(1));
    second
        .maintenance
        .send(Maintain::ReleaseSettlement)
        .unwrap();
    for _ in 0..300 {
        assert!(!final_custody_step(&second).completed);
    }
    assert_eq!(owner.custodies_kept(), 1);
    first.finish();
    second.finish();
}

#[test]
fn final_custody_preserves_unreadable_store_without_completion_or_capacity_return() {
    let owner = Arc::new(service_owner(&PrivateSettlementOwner::with_capacity(16), 4));
    let (service, custody) = closed_custody_service(owner.clone(), false);
    for _ in 0..100 {
        assert!(!final_custody_step(&service).completed);
        if custody
            .cleanup_record()
            .ordered_home
            .storage_returned
            .load(Ordering::Acquire)
        {
            break;
        }
    }
    assert!(
        custody
            .cleanup_record()
            .ordered_home
            .storage_returned
            .load(Ordering::Acquire)
    );
    let store = owner.store.clone();
    assert!(
        std::thread::spawn(move || {
            let _held = store.inner.lock().unwrap();
            panic!("labelled store poison after original output storage return");
        })
        .join()
        .is_err()
    );
    for _ in 0..32 {
        assert!(!final_custody_step(&service).completed);
    }
    assert_eq!(owner.custodies_kept(), 1);
    assert!(owner.store.inner.is_poisoned());
    service.finish();
}

#[test]
fn final_completion_keeps_original_internal_home_when_external_custody_is_withheld() {
    let owner = Arc::new(service_owner(&PrivateSettlementOwner::with_capacity(16), 4));
    let (service, custody) = closed_custody_service(owner.clone(), false);
    assert!(
        !custody
            .cleanup_record()
            .ordered_home
            .storage_returned
            .load(Ordering::Acquire)
    );
    let (index, original) = {
        let mut kept = owner.inventory.kept.lock().unwrap();
        let index = kept
            .places
            .iter()
            .position(|slot| {
                slot.as_ref()
                    .is_some_and(|candidate| Arc::ptr_eq(candidate, &custody))
            })
            .unwrap();
        (index, kept.places[index].take().unwrap())
    };
    for _ in 0..400 {
        assert!(
            !final_custody_step(&service).completed,
            "the original internal continuation still owns output despite a missing external row"
        );
    }
    assert_eq!(owner.store.inner.lock().unwrap().continuation_slots, 1);
    owner.inventory.kept.lock().unwrap().places[index] = Some(original);
    wait_final_custody(&service);
    service.finish();
}
