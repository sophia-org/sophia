//! Real pre-use rollback receipts exercise the owner/journal lifetime seam.
use super::*;
use crate::shell_capture_owner::Publication;
use sophia_protocol::shell_files::ShellSourceDisposition;
use sophia_runtime::ShellTransportError;

#[test]
fn observation_error_names_its_exact_connection_not_the_first_pending_job() {
    let mut f = Fixture::new();
    let first = binding(1, 1);
    let second = binding(2, 1);
    f.submit(first);
    f.submit(second);
    // This registry knows only the first custody. Missing second owner must
    // never report a failure against the valid first connection.
    let errors = f.owner.service(&mut f.registries[0], 0, f.now, |_, _| {
        panic!("executor has not serviced requests")
    });
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, second);
    assert_eq!(f.owner.pending(), 2);
}

#[test]
fn journal_pressure_retains_source_and_slot_without_repeating_settlement() {
    let mut f = Fixture::new();
    let b = binding(1, 1);
    let (_, fd) = f.submit(b);
    drop(f.executor.take());
    f.owner.stop();
    assert!(!f.owner.replaceable());
    for _ in 0..3 {
        let errors = f
            .owner
            .service(&mut f.registries[0], 0, f.now, |got, disposition| {
                assert_eq!(got, b);
                assert_eq!(disposition, ShellSourceDisposition::CancelledBeforeImport);
                Err(ShellTransportError::ContentQueueSaturated)
            });
        assert!(errors.is_empty());
        assert_eq!(f.owner.pending(), 1);
        assert!(
            !f.owner.replaceable(),
            "journal pressure retains closed owner"
        );
        assert!(!f.owner.awaiting_completion());
        assert_eq!(f.budget.usage(), (1, 64));
        let held = f.owner.settling.iter().flatten().next().unwrap();
        assert!(held.reservation.is_none());
        let LiveShellCaptureOutcome::Returned { request, .. } = held.receipt.outcome() else {
            panic!()
        };
        assert_eq!(request.source.fd.as_raw_fd(), fd);
    }
    let resources = f.registries[0].resources_mut(b.grant).unwrap();
    assert!(matches!(
        resources.take_event().map(|event| event.record),
        Some(ShellContentRecord::ResourceStatus(_))
    ));
    assert!(
        resources.take_event().is_none(),
        "settlement must happen exactly once"
    );
    assert!(
        f.owner
            .service(&mut f.registries[0], 0, f.now, |_, _| Ok(
                Publication::Journaled
            ))
            .is_empty()
    );
    assert_eq!(f.owner.pending(), 0);
    assert!(f.owner.replaceable());
    assert_eq!(f.budget.usage(), (0, 0));
}

#[test]
fn publication_error_is_not_disconnection_proof() {
    let mut f = Fixture::new();
    f.submit(binding(1, 1));
    drop(f.executor.take());
    let errors = f.owner.service(&mut f.registries[0], 0, f.now, |_, _| {
        Err(ShellTransportError::NotConnected)
    });
    assert_eq!(errors.len(), 1);
    assert_eq!(f.owner.pending(), 1);
    assert_eq!(f.budget.usage(), (1, 64));
    assert!(
        f.owner
            .service(&mut f.registries[0], 0, f.now, |_, _| Ok(
                Publication::Disconnected
            ))
            .is_empty()
    );
    assert_eq!(f.owner.pending(), 0);
    assert_eq!(f.budget.usage(), (0, 0));
}

#[test]
fn clock_refusal_preserves_receipt_and_ticket_before_any_publication() {
    let mut f = Fixture::new();
    let b = binding(1, 1);
    f.submit(b);
    f.registries[0]
        .resources_mut(b.grant)
        .unwrap()
        .expire(10)
        .unwrap();
    drop(f.executor.take());
    assert!(
        !f.owner
            .service(&mut f.registries[0], 9, f.now, |_, _| panic!(
                "no publication before settlement"
            ))
            .is_empty()
    );
    assert_eq!(f.owner.pending(), 1);
    assert!(
        f.owner
            .settling
            .iter()
            .flatten()
            .next()
            .unwrap()
            .reservation
            .is_some()
    );
    assert_eq!(f.budget.usage(), (1, 64));
    assert!(
        f.owner
            .service(&mut f.registries[0], 10, f.now, |_, _| Ok(
                Publication::Journaled
            ))
            .is_empty()
    );
    assert_eq!(f.owner.pending(), 0);
}

#[test]
fn journal_waiters_still_consume_all_four_capture_slots() {
    let mut f = Fixture::new();
    let mut registry = allocation_registry(binding(1, 1).grant);
    for resource in 1..=4 {
        let b = binding(1, resource);
        let ticket = registry
            .reserve_capture_image(b.grant, &b.metadata, 0)
            .unwrap();
        f.owner
            .submit(b, f.request(), ticket, f.now + Duration::from_secs(10))
            .unwrap();
    }
    drop(f.executor.take());
    assert!(
        f.owner
            .service(&mut registry, 0, f.now, |_, _| Err(
                ShellTransportError::ContentQueueSaturated
            ))
            .is_empty()
    );
    assert_eq!(f.owner.pending(), 4);
    let b = binding(1, 5);
    let ticket = registry
        .reserve_capture_image(b.grant, &b.metadata, 0)
        .unwrap();
    let error = f
        .owner
        .submit(b, f.request(), ticket, f.now + Duration::from_secs(10))
        .unwrap_err();
    assert_eq!(error.reason, SubmitFailure::Capacity);
    assert_eq!(f.owner.pending(), 4);
}
