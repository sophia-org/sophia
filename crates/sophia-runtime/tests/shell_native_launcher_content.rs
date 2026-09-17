use sophia_protocol::*;
use sophia_runtime::*;
#[path = "support/native_launcher_content.rs"]
mod support;
use support::*;

#[test]
fn native_allocation_retains_exact_opening_and_cannot_claim_panel_reservation() {
    let mut r = registry();
    let s = r.allocations_mut(GRANT).unwrap();
    s.request_native_launcher(tx(1), request(1), opening(), 0)
        .unwrap();
    assert_eq!(s.pending_native_opening(1), Some(7));
    let mut wrong = allocation();
    wrong.native_opening = Some(8);
    assert_eq!(s.grant(1, wrong, &[]), Err(ContentAllocationError::Stale));
    assert!(s.pending_request().is_some());
    let mut wrong = allocation();
    wrong.allowed_reservation_extent = 1;
    assert_eq!(
        s.grant(1, wrong, &[]),
        Err(ContentAllocationError::Malformed)
    );
    let mut wrong = allocation();
    wrong.pixel.x += 1;
    assert_eq!(
        s.grant(1, wrong, &[]),
        Err(ContentAllocationError::Malformed)
    );
    s.grant(1, allocation(), &[]).unwrap();
    let event = s.take_event().unwrap();
    let ShellContentRecord::AllocationResult(result) = event.record else {
        panic!()
    };
    assert_eq!(event.transaction, tx(1));
    assert_eq!(result.allowed_reservation_extent, 0);
    assert_eq!(result.parent, ContentAllocationId::default());
    assert_eq!(result.acknowledged_anchor, ContentPixelRect::default());
    assert_eq!(s.snapshots(), vec![allocation()]);
    assert_eq!(
        s.request_native_launcher(tx(2), request(2), opening(), 0),
        Err(ContentAllocationError::Budget)
    );
    assert!(s.take_event().is_none());
}

#[test]
fn stale_opening_cannot_resize_or_release_another_allocation() {
    let mut r = registry();
    grant_allocation(&mut r);
    let s = r.allocations_mut(GRANT).unwrap();
    let mut v = request(2);
    v.operation = 2;
    v.prior = ALLOCATION;
    let mut newer = opening();
    newer.opening = 8;
    v.opening = 8;
    assert_eq!(
        s.request_native_launcher(tx(2), v, newer, 0),
        Err(ContentAllocationError::Stale)
    );
    assert_eq!(s.snapshots(), vec![allocation()]);
    v.opening = 7;
    s.request_native_launcher(tx(3), v, opening(), 0).unwrap();
    let mut replacement = allocation();
    replacement.allocation.generation = 2;
    s.grant(2, replacement.clone(), &[]).unwrap();
    s.take_event().unwrap();
    let mut release = request(3);
    release.operation = 3;
    release.prior = ALLOCATION;
    release.desired_width = 0;
    release.desired_height = 0;
    assert_eq!(
        s.request_native_launcher(tx(4), release, opening(), 0),
        Err(ContentAllocationError::AllocationLost)
    );
    release.request_id = 4;
    release.prior = replacement.allocation;
    s.request_native_launcher(tx(5), release, opening(), 0)
        .unwrap();
    s.release(4).unwrap();
    assert!(s.snapshots().is_empty());
}

#[test]
fn legacy_profiles_and_legacy_entry_points_do_not_admit_native_content() {
    let mut legacy = ContentCandidateStore::new(limits()).unwrap();
    let c = catalog();
    assert_eq!(
        legacy.begin_native_launcher(tx(1), begin(), native(&c), 0),
        Err(ContentCandidateError::Malformed)
    );
    let mut r = registry();
    let native_store = r.active_candidates_mut(GRANT).unwrap();
    assert_eq!(
        native_store.begin(tx(1), begin().content, 0),
        Err(ContentCandidateError::Malformed)
    );
    assert_eq!(
        native_store.chunk(tx(1), chunk(), 0),
        Err(ContentCandidateError::Malformed)
    );
    let mut a = ContentAllocationStore::new(limits()).unwrap();
    assert_eq!(
        a.request_native_launcher(tx(1), request(1), opening(), 0),
        Err(ContentAllocationError::Stale)
    );
    assert_eq!(r.profile(GRANT), Some(ContentStoreProfile::NativeLauncher));
}

#[test]
fn rows_survive_actual_assembly_and_submission_while_real_bytes_remain_owned() {
    let mut r = registry();
    let a = grant_allocation(&mut r);
    let c = catalog();
    assemble(&mut r, &a, &c);
    // One ResourceReleased credit remains alongside the two candidate outcomes.
    assert_eq!(r.accounting().response_records, 3);
    let s = r.active_candidates_mut(GRANT).unwrap();
    let bundle = s
        .begin_native_launcher_submission(1, context(&a), native(&c), 0)
        .unwrap();
    let binding = bundle.native_launcher.unwrap();
    assert_eq!(binding.rows(), &[2, 1]);
    assert_eq!(binding.selected, 2);
    assert_eq!(binding.opening, 7);
    assert_eq!(binding.catalog_generation, 8);
    assert_eq!(binding.state_revision, 1);
    assert_eq!(
        bundle.resource(RESOURCE).unwrap().bytes(),
        &[0, 0, 255, 255, 0, 128, 0, 128]
    );
    // Supplied renderer boundaries, not a native presentation observation.
    s.prepared(OUTPUT, 1, 1, 1, 0).unwrap();
    s.presented(OUTPUT, 1, 9, 1, 1).unwrap();
    let events: Vec<_> = std::iter::from_fn(|| s.take_event()).collect();
    assert_eq!(events.len(), 2);
    for (event, kind) in events.iter().zip([1, 2]) {
        assert_eq!(event.transaction, tx(6));
        assert!(
            matches!(event.record, ShellContentRecord::CandidateOutcome(ref v) if v.kind==kind && v.candidate_generation==1)
        );
    }
    assert!(r.disconnect(GRANT));
    r.collect();
    assert_eq!(r.accounting().retired_epochs, 1);
    assert_eq!(r.accounting().memory.resident, 8);
    assert_eq!(bundle.native_launcher, Some(binding));
    drop(bundle);
    r.collect();
    assert_eq!(r.accounting().retired_epochs, 0);
    assert_eq!(r.reserved_bytes(), 0);
}

#[test]
fn changed_catalog_or_revision_before_submission_keeps_actual_pending_owner() {
    let mut r = registry();
    let a = grant_allocation(&mut r);
    let c = catalog();
    assemble(&mut r, &a, &c);
    let s = r.active_candidates_mut(GRANT).unwrap();
    let mut updated = native(&c);
    updated.state_revision = 2;
    assert!(matches!(
        s.begin_native_launcher_submission(1, context(&a), updated, 0),
        Err(ContentCandidateError::Stale)
    ));
    let mut other = c.clone();
    other.generation += 1;
    assert!(matches!(
        s.begin_native_launcher_submission(1, context(&a), native(&other), 0),
        Err(ContentCandidateError::Stale)
    ));
    let mut wrong = a.clone();
    wrong[0].native_opening = Some(8);
    assert!(matches!(
        s.begin_native_launcher_submission(1, context(&wrong), native(&c), 0),
        Err(ContentCandidateError::AllocationLost)
    ));
    assert_eq!(s.pending_candidate_count(), 1);
    assert_eq!(s.submitted_candidate_count(), 0);
    assert!(s.take_event().is_none());
    assert!(
        s.begin_native_launcher_submission(1, context(&a), native(&c), 0)
            .is_ok()
    );
}

#[test]
fn unknown_unavailable_or_duplicate_catalog_rows_settle_one_begin_debt() {
    for mode in 0..3 {
        let mut r = registry();
        let mut c = catalog();
        if mode == 0 {
            c.entries.pop();
        }
        if mode == 1 {
            c.entries[1].available = false;
        }
        if mode == 2 {
            c.entries.push(c.entries[1].clone());
        }
        let s = r.active_candidates_mut(GRANT).unwrap();
        s.grant_permit(tx(1), OUTPUT, 1, 1, 0).unwrap();
        s.take_event().unwrap();
        assert_eq!(
            s.begin_native_launcher(tx(2), begin(), native(&c), 0),
            Err(ContentCandidateError::Malformed)
        );
        assert!(
            matches!(s.take_event().unwrap().record, ShellContentRecord::CandidateOutcome(v) if v.kind==3 && v.candidate_generation==1)
        );
        assert!(s.take_event().is_none());
        assert!(s.quiescent());
        // The uploaded resource still owns its eventual ResourceReleased credit.
        assert_eq!(r.accounting().response_records, 1);
    }
}

#[test]
fn end_requires_exact_catalog_order_opening_and_current_revision() {
    for mode in 0..4 {
        let mut r = registry();
        let mut a = grant_allocation(&mut r);
        let c = catalog();
        let (resources, s) = r.active_parts_mut(GRANT).unwrap();
        s.grant_permit(tx(1), OUTPUT, 1, 1, 0).unwrap();
        s.take_event().unwrap();
        s.begin_native_launcher(tx(2), begin(), native(&c), 0)
            .unwrap();
        let mut ch = chunk();
        if mode == 0 {
            ch.targets.reverse();
        }
        s.chunk_native_launcher(tx(3), ch, 0).unwrap();
        let mut current = native(&c);
        if mode == 1 {
            a[0].native_opening = Some(8);
        }
        if mode == 2 {
            current.state_revision = 2;
        }
        if mode == 3 {
            current.opening.opening = 8;
        }
        assert!(
            s.end_native_launcher(tx(4), end(), context(&a), current, resources, 0)
                .is_err()
        );
        let event = s.take_event().unwrap();
        assert_eq!(event.transaction, tx(2));
        assert!(matches!(event.record, ShellContentRecord::CandidateOutcome(v) if v.kind==3));
        assert!(s.take_event().is_none());
        assert!(s.quiescent());
    }
}

#[test]
fn native_begin_and_rows_are_in_the_candidate_byte_budget() {
    for budget in [343, 344] {
        let mut limits = limits();
        limits.max_candidate_bytes = budget;
        let mut s =
            ContentCandidateStore::with_profile(limits, ContentStoreProfile::NativeLauncher)
                .unwrap();
        let c = catalog();
        s.grant_permit(tx(1), OUTPUT, 1, 1, 0).unwrap();
        s.take_event().unwrap();
        s.begin_native_launcher(tx(2), begin(), native(&c), 0)
            .unwrap();
        let outcome = s.chunk_native_launcher(tx(3), chunk(), 0);
        assert_eq!(outcome.is_ok(), budget == 344);
        if budget == 343 {
            assert!(s.take_event().is_some());
            assert!(s.quiescent());
        }
    }
}

#[test]
fn begin_metadata_cannot_exceed_candidate_budget_before_any_chunk() {
    let mut limits = limits();
    limits.max_candidate_bytes = 151;
    let mut s =
        ContentCandidateStore::with_profile(limits, ContentStoreProfile::NativeLauncher).unwrap();
    let c = catalog();
    s.grant_permit(tx(1), OUTPUT, 1, 1, 0).unwrap();
    s.take_event().unwrap();
    assert_eq!(
        s.begin_native_launcher(tx(2), begin(), native(&c), 0),
        Err(ContentCandidateError::Budget)
    );
    assert!(
        matches!(s.take_event().unwrap().record, ShellContentRecord::CandidateOutcome(v)
        if v.kind==3 && v.reason==ContentReason::Budget as u16)
    );
    assert!(s.quiescent());
    assert!(s.take_event().is_none());
}

#[test]
fn revoked_native_submission_keeps_its_bytes_while_the_bar_uploads_and_replacement_waits() {
    let mut r = registry_with_bar(true);
    let a = grant_allocation(&mut r);
    let c = catalog();
    assemble(&mut r, &a, &c);
    let s = r.active_candidates_mut(GRANT).unwrap();
    let bundle = s
        .begin_native_launcher_submission(1, context(&a), native(&c), 0)
        .unwrap();
    s.prepared(OUTPUT, 1, 1, 1, 0).unwrap();
    assert!(r.disconnect(GRANT));
    assert!(r.active_candidates(GRANT).is_none());
    let bar = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    upload(&mut r, bar, 37);
    let neighbor = r.resources(bar).unwrap().lease(bar, RESOURCE).unwrap();
    assert_eq!(neighbor.bytes()[2], 37);
    assert_eq!(bundle.resource(RESOURCE).unwrap().bytes()[2], 255);
    let mut next = limits();
    next.grant = ContentGrant {
        connection_epoch: 3,
        content_grant_epoch: 3,
    };
    assert_eq!(
        r.admit_with_profile(next.clone(), ContentStoreProfile::NativeLauncher),
        Err(ContentStoreError::Budget)
    );
    // A supplied failure settles the old submitted candidate, not its distinct
    // retained renderer consumer. No native callback or bar action is simulated.
    r.candidates_mut(GRANT)
        .unwrap()
        .renderer_failed(OUTPUT, 1)
        .unwrap();
    r.collect();
    assert_eq!(r.accounting().retired_epochs, 1);
    assert_eq!(r.reserved_bytes(), 40 * 1024 * 1024 + 8);
    assert_eq!(
        r.admit_with_profile(next.clone(), ContentStoreProfile::NativeLauncher),
        Err(ContentStoreError::Budget)
    );
    drop(bundle);
    r.collect();
    assert_eq!(r.accounting().retired_epochs, 0);
    r.admit_with_profile(next.clone(), ContentStoreProfile::NativeLauncher)
        .unwrap();
    assert_eq!(
        r.profile(next.grant),
        Some(ContentStoreProfile::NativeLauncher)
    );
    assert_eq!(r.profile(bar), Some(ContentStoreProfile::Legacy));
    assert_eq!(neighbor.bytes()[2], 37);
}
