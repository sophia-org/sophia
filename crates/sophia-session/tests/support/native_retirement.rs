//! Shared production retirement state with simulated worker/disposition effects.
use super::*;
use std::{cell::Cell, rc::Rc, sync::Arc};

struct Owner {
    id: u64,
    joined: Rc<Cell<bool>>,
    disposed: Rc<Cell<bool>>,
    failed: Rc<Cell<bool>>,
    requested: Rc<Cell<usize>>,
    bytes: Arc<Vec<u8>>,
}

impl RetirementOwner for Owner {
    fn identity(&self) -> u64 {
        self.id
    }
    fn request_shutdown(&self) {
        self.requested.set(self.requested.get() + 1);
    }
    fn poll_shutdown(&self) -> Result<bool, Box<dyn std::error::Error>> {
        if self.failed.get() {
            return Err("worker join failed".into());
        }
        Ok(self.joined.get())
    }
    fn disposition(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.disposed.get() {
            return Err("scanout disposition unresolved".into());
        }
        Ok(())
    }
}

fn owner(id: u64) -> Owner {
    Owner {
        id,
        joined: Rc::new(Cell::new(false)),
        disposed: Rc::new(Cell::new(false)),
        failed: Rc::new(Cell::new(false)),
        requested: Rc::new(Cell::new(0)),
        bytes: Arc::new(vec![42; 4096]),
    }
}

#[test]
fn seat_release_drives_exact_owner_before_broker_acknowledgement() {
    use sophia_backend_live::LiveSeatDisableOutcome as Outcome;
    let value = owner(91);
    let joined = value.joined.clone();
    let bytes = value.bytes.clone();
    value.disposed.set(true);
    let mut state = NativeRetirement::default();
    state
        .begin(&mut Some(value), RetirementMode::Drained, "vt")
        .unwrap();
    for _ in 0..3 {
        assert_eq!(
            state
                .poll_seat_release(|| panic!("premature disable"))
                .unwrap(),
            None
        );
        assert_eq!(Arc::strong_count(&bytes), 2);
    }
    joined.set(true);
    assert_eq!(
        state
            .poll_seat_release(|| {
                assert_eq!(
                    Arc::strong_count(&bytes),
                    1,
                    "actual owner dropped before broker request"
                );
                Ok(Outcome::Pending { leases: 1 })
            })
            .unwrap(),
        Some(Outcome::Pending { leases: 1 })
    );
    assert_eq!(
        state
            .poll_seat_release(|| Ok(Outcome::Acknowledged))
            .unwrap(),
        Some(Outcome::Acknowledged)
    );
    assert_eq!(state.completion().unwrap().identity, 91);
    let next = owner(92);
    state.admit(&next).unwrap();
    assert!(
        state
            .poll_seat_release(|| panic!("old completion authorized successor"))
            .is_err()
    );
}

#[test]
fn revoked_unresolved_owner_and_timeout_survive_release_without_ack() {
    let value = owner(93);
    let joined = value.joined.clone();
    let bytes = value.bytes.clone();
    let mut state = NativeRetirement::default();
    state
        .begin(&mut Some(value), RetirementMode::DeviceRevoked, "disable")
        .unwrap();
    state.pending.as_mut().unwrap().started = Instant::now() - Duration::from_secs(3);
    assert!(
        state
            .poll_seat_release(|| panic!("timeout acknowledged"))
            .is_err()
    );
    joined.set(true);
    assert!(
        state
            .poll_seat_release(|| panic!("unresolved scanout acknowledged"))
            .is_err()
    );
    assert_eq!(Arc::strong_count(&bytes), 2);
    assert!(state.completion().is_err());
}

#[test]
fn final_accounting_requires_exact_completion_even_with_empty_slots() {
    let mut state = NativeRetirement::<Owner>::default();
    assert!(state.finish_completed().is_err());
    let value = owner(70);
    value.joined.set(true);
    value.disposed.set(true);
    state
        .begin(&mut Some(value), RetirementMode::Drained, "final")
        .unwrap();
    assert_eq!(state.finish_completed().unwrap().identity, 70);
    let successor = owner(71);
    state.admit(&successor).unwrap();
    assert!(state.finish_completed().is_err());
}

#[test]
fn retirement_keeps_pending_result_bytes_until_join_and_disposition() {
    let value = owner(1);
    let (joined, disposed, bytes) = (
        value.joined.clone(),
        value.disposed.clone(),
        value.bytes.clone(),
    );
    let mut live = Some(value);
    let mut state = NativeRetirement::default();
    state
        .begin(&mut live, RetirementMode::Drained, "final")
        .unwrap();
    assert!(live.is_none());
    assert!(!state.poll().unwrap());
    assert_eq!(Arc::strong_count(&bytes), 2);
    joined.set(true);
    assert!(state.poll().is_err());
    assert!(state.completion().is_err());
    assert_eq!(Arc::strong_count(&bytes), 2);
    disposed.set(true);
    assert!(state.poll().unwrap());
    assert_eq!(state.completion().unwrap().identity, 1);
    assert_eq!(Arc::strong_count(&bytes), 1);
    assert!(state.poll().unwrap());
}

#[test]
fn revoked_join_does_not_mint_drained_or_dispose_unresolved_scanout() {
    let value = owner(2);
    value.joined.set(true);
    let bytes = value.bytes.clone();
    let disposition = value.disposed.clone();
    let mut live = Some(value);
    let mut state = NativeRetirement::default();
    state
        .begin(&mut live, RetirementMode::DeviceRevoked, "revoked")
        .unwrap();
    assert!(state.poll().is_err());
    assert_eq!(Arc::strong_count(&bytes), 2);
    assert!(state.completion().is_err());
    disposition.set(true);
    state.poll().unwrap();
    assert_eq!(
        state.completion().unwrap().mode,
        RetirementMode::DeviceRevoked
    );
    assert_eq!(Arc::strong_count(&bytes), 1);
}

#[test]
fn repeated_retire_and_replacement_refusal_keep_exact_old_owner() {
    let value = owner(3);
    let requests = value.requested.clone();
    let mut live = Some(value);
    let mut state = NativeRetirement::default();
    state
        .begin(&mut live, RetirementMode::Drained, "vt")
        .unwrap();
    state
        .begin(&mut live, RetirementMode::DeviceRevoked, "repeat")
        .unwrap();
    assert_eq!(requests.get(), 1);
    let mut successor = Some(owner(4));
    assert!(
        state
            .begin(&mut successor, RetirementMode::Drained, "replacement")
            .is_err()
    );
    assert_eq!(successor.as_ref().unwrap().id, 4);
    assert_eq!(state.pending.as_ref().unwrap().owner.id, 3);
}

#[test]
fn completed_previous_owner_does_not_authorize_resumed_owner() {
    let value = owner(5);
    value.joined.set(true);
    value.disposed.set(true);
    let mut live = Some(value);
    let mut state = NativeRetirement::default();
    state
        .begin(&mut live, RetirementMode::Drained, "vt")
        .unwrap();
    state.poll().unwrap();
    assert_eq!(state.completion().unwrap().identity, 5);
    let next = owner(6);
    state.admit(&next).unwrap();
    assert!(state.completion().is_err());
    assert!(!state.pending());
}

#[test]
fn timeout_and_failed_join_preserve_owner_across_terminal_error_carrier() {
    let value = owner(7);
    let (bytes, failed) = (value.bytes.clone(), value.failed.clone());
    let mut live = Some(value);
    let mut state = NativeRetirement::default();
    state
        .begin(&mut live, RetirementMode::Drained, "loop_error")
        .unwrap();
    state.pending.as_mut().unwrap().started = Instant::now() - Duration::from_secs(3);
    assert!(state.poll().unwrap_err().to_string().contains("timed out"));
    failed.set(true);
    assert!(
        state
            .poll()
            .unwrap_err()
            .to_string()
            .contains("join failed")
    );
    let accounting = Arc::new(vec![17u8]);
    let weak = Arc::downgrade(&accounting);
    let error: Box<dyn std::error::Error> = Box::new(RetirementFailure::new(
        "unrelated frontend failure".into(),
        state,
        accounting,
    ));
    assert_eq!(error.to_string(), "unrelated frontend failure");
    assert_eq!(Arc::strong_count(&bytes), 2);
    assert!(weak.upgrade().is_some());
    // Only explicit terminal failure teardown destroys the carrier; this is
    // not a Completed observation or a successful protocol resource release.
    drop(error);
    assert_eq!(Arc::strong_count(&bytes), 1);
    assert!(weak.upgrade().is_none());
}

#[test]
fn absent_native_without_a_completed_owner_is_not_shutdown_evidence() {
    let mut state = NativeRetirement::<Owner>::default();
    state
        .begin(&mut None, RetirementMode::Drained, "never_owned")
        .unwrap();
    assert!(state.poll().unwrap());
    assert!(state.completion().is_err());
}

#[test]
fn completed_abandoned_owner_does_not_report_drained() {
    use sophia_backend_live::LiveProductionNativeSuspendOutcome as Outcome;
    for outcome in [
        Outcome::ForcedDetachTimeout,
        Outcome::ForcedDetachDrainError,
    ] {
        let value = owner(9);
        value.joined.set(true);
        value.disposed.set(true);
        let mut live = Some(value);
        let mut state = NativeRetirement::default();
        state
            .begin(
                &mut live,
                RetirementMode::from_suspend(outcome),
                "forced_detach",
            )
            .unwrap();
        state.poll().unwrap();
        assert_eq!(state.completion().unwrap().mode, RetirementMode::Abandoned);
    }
}

struct RenderOwner {
    _bytes: Arc<Vec<u8>>,
    drains: Rc<Cell<usize>>,
    disposed: Rc<Cell<bool>>,
}

impl RenderRetirement<Owner> for RenderOwner {
    fn drain(&mut self, _: &mut Owner) -> Result<(), Box<dyn std::error::Error>> {
        self.drains.set(self.drains.get() + 1);
        Ok(())
    }

    fn disposition(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.disposed.get() {
            Ok(())
        } else {
            Err("visual owner still holds scanout".into())
        }
    }
}

#[test]
fn final_exit_preserves_all_owners_through_prior_error_and_revoked_refusal() {
    for active in [true, false] {
        let bytes = Arc::new(vec![12; 8192]);
        let drains = Rc::new(Cell::new(0));
        let disposed = Rc::new(Cell::new(false));
        let mut runtime = Some(RenderOwner {
            _bytes: bytes.clone(),
            drains: drains.clone(),
            disposed: disposed.clone(),
        });
        let mut scene = Some(bytes.clone());
        let owner = owner(80);
        owner.joined.set(true);
        owner.disposed.set(true);
        let requests = owner.requested.clone();
        let native_bytes = owner.bytes.clone();
        let mut native = Some(owner);
        let mut retirement = NativeRetirement::default();
        let error = finish_render_owners(
            &mut runtime,
            &mut scene,
            &mut native,
            &mut retirement,
            active,
            true,
        )
        .unwrap_err();
        assert_eq!(drains.get(), usize::from(active));
        assert_eq!(requests.get(), 1);
        assert!(native.is_none());
        assert!(retirement.completion().is_err());
        assert_eq!(Arc::strong_count(&bytes), 3);
        assert_eq!(Arc::strong_count(&native_bytes), 2);
        let prior: Box<dyn std::error::Error> = "unrelated owner-loop failure".into();
        let failure: Box<dyn std::error::Error> = Box::new(RetirementFailure::new(
            error.to_string(),
            retirement,
            (runtime, scene, native, prior),
        ));
        assert!(failure.to_string().contains("visual owner"));
        assert_eq!(Arc::strong_count(&bytes), 3);
        assert_eq!(Arc::strong_count(&native_bytes), 2);
        let mut failure = failure
            .downcast::<RetirementFailure<
                Owner,
                (
                    Option<RenderOwner>,
                    Option<Arc<Vec<u8>>>,
                    Option<Owner>,
                    Box<dyn std::error::Error>,
                ),
            >>()
            .unwrap();
        disposed.set(true);
        let (runtime, scene, native, prior) = &mut failure._held;
        assert_eq!(prior.to_string(), "unrelated owner-loop failure");
        let receipt = finish_render_owners(
            runtime,
            scene,
            native,
            &mut failure.retirement,
            active,
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(receipt.identity, 80);
        assert_eq!(
            receipt.mode,
            if active {
                RetirementMode::Drained
            } else {
                RetirementMode::DeviceRevoked
            }
        );
        assert_eq!(drains.get(), usize::from(active));
        assert_eq!(requests.get(), 1);
        assert_eq!(Arc::strong_count(&bytes), 1);
        assert_eq!(Arc::strong_count(&native_bytes), 1);
    }
}

#[test]
fn explicit_headless_exit_is_not_a_native_completion_receipt() {
    let mut retirement = NativeRetirement::<Owner>::default();
    let mut runtime = None::<RenderOwner>;
    let mut scene = Some(Arc::new(vec![1_u8; 16]));
    let mut native = None;
    assert!(
        finish_render_owners(
            &mut runtime,
            &mut scene,
            &mut native,
            &mut retirement,
            false,
            false,
        )
        .unwrap()
        .is_none()
    );
    assert!(scene.is_none());
    assert!(retirement.completion().is_err());
    assert!(
        finish_render_owners(
            &mut runtime,
            &mut scene,
            &mut native,
            &mut retirement,
            false,
            true,
        )
        .is_err()
    );
    native = Some(owner(91));
    assert!(
        finish_render_owners(
            &mut runtime,
            &mut scene,
            &mut native,
            &mut retirement,
            false,
            false,
        )
        .is_err()
    );
    assert_eq!(native.as_ref().unwrap().id, 91);
}

#[test]
fn resume_failure_keeps_handoff_until_entire_restore_succeeds() {
    let bytes = Arc::new(vec![3_u8; 1024]);
    let mut handoff = Some(bytes.clone());
    for _ in 0..2 {
        let error = restore_retained_handoff(&mut handoff, |source| {
            assert!(Arc::ptr_eq(source.unwrap(), &bytes));
            Err::<(), _>("replacement rejected")
        });
        assert_eq!(error, Err("replacement rejected"));
        assert_eq!(Arc::strong_count(&bytes), 2);
    }
    let restored = restore_retained_handoff(&mut handoff, |source| {
        Ok::<_, &'static str>(source.unwrap().clone())
    })
    .unwrap();
    assert!(handoff.is_none());
    assert_eq!(Arc::strong_count(&bytes), 2);
    drop(restored);
    assert_eq!(Arc::strong_count(&bytes), 1);
}

#[test]
fn absent_retiring_slot_does_not_authorize_a_successor_of_a_live_owner() {
    let original = owner(101);
    let successor = owner(102);
    let mut retirement = NativeRetirement::default();
    retirement.admit(&original).unwrap();
    assert!(!retirement.pending());
    assert!(retirement.admit(&successor).is_err());
    assert_eq!(retirement.latest, Some(101));
    assert!(retirement.completion().is_err());
    assert_eq!(original.id, 101);
    assert_eq!(successor.requested.get(), 0);
}

#[test]
fn pre_return_completion_requires_device_authority_and_keeps_owners_on_revocation() {
    for active in [false, true] {
        let bytes = Arc::new(vec![1_u8; 2048]);
        let mut held = Some(bytes.clone());
        let mut calls = Vec::new();
        // The production completion block uses this same gate before topology,
        // drain, and presentation release. Effects here are supplied, not KMS.
        let result = with_active_device_authority(active, || {
            calls.extend(["topology", "drain", "release"]);
            held.take()
        });
        if active {
            assert_eq!(calls, ["topology", "drain", "release"]);
            assert!(held.is_none());
            drop(result);
            assert_eq!(Arc::strong_count(&bytes), 1);
        } else {
            assert!(calls.is_empty());
            assert!(result.is_none());
            assert!(held.is_some());
            assert_eq!(Arc::strong_count(&bytes), 2);
        }
    }
}

#[test]
fn replacement_waits_for_retained_runtime_even_after_native_join() {
    let value = owner(103);
    value.joined.set(true);
    value.disposed.set(true);
    let bytes = value.bytes.clone();
    let mut retirement = NativeRetirement::default();
    retirement
        .begin(&mut Some(value), RetirementMode::DeviceRevoked, "revoked")
        .unwrap();
    let disposed = Rc::new(Cell::new(false));
    let runtime = RenderOwner {
        _bytes: Arc::new(vec![2; 1024]),
        drains: Rc::new(Cell::new(0)),
        disposed: disposed.clone(),
    };
    assert!(finish_before_replacement(Some(&runtime), &mut retirement).is_err());
    assert_eq!(Arc::strong_count(&bytes), 2);
    assert!(retirement.completion().is_err());
    disposed.set(true);
    finish_before_replacement(Some(&runtime), &mut retirement).unwrap();
    assert_eq!(retirement.completion().unwrap().identity, 103);
    assert_eq!(Arc::strong_count(&bytes), 1);
    assert_eq!(runtime.drains.get(), 0);
}
