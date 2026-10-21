use super::super::{NativeCaptureAllocationAccounting, NativeCaptureAllocationCharge};
use super::*;
use std::cell::Cell;

type Proof = CompletedCopy<NativeCaptureAllocationCharge, usize>;

struct Fixture {
    accounting: Rc<NativeCaptureAllocationAccounting>,
    allocation: Rc<NativeCaptureAllocationCharge>,
    generation: Rc<NativeSnapshotGeneration>,
    guard: NativeSnapshotUse,
    state: Observation,
}

impl Fixture {
    fn new() -> Self {
        let accounting = Rc::new(NativeCaptureAllocationAccounting::default());
        let allocation = NativeCaptureAllocationCharge::new(&accounting, 4096);
        let generation = NativeSnapshotGeneration::new(1, 1);
        let guard = generation.acquire_use();
        Self {
            accounting,
            allocation,
            generation,
            guard,
            state: Observation::default(),
        }
    }

    fn observe(&mut self, result: Result<khronos_egl::Int, ()>) -> Result<Option<Proof>, Refusal> {
        self.state.observe(
            &self.allocation,
            &self.generation,
            Some(17),
            Some(&self.guard),
            |sync| {
                assert_eq!(sync, 17);
                result
            },
        )
    }
}

#[test]
fn timeout_keeps_the_same_copy_pending_and_completion_is_issued_once() {
    let mut fixture = Fixture::new();
    assert!(
        fixture
            .observe(Ok(khronos_egl::TIMEOUT_EXPIRED))
            .unwrap()
            .is_none()
    );
    assert_eq!(fixture.state, Observation::Pending);
    assert_eq!(fixture.generation.pending_uses.get(), 1);
    assert_eq!(fixture.accounting.bytes.get(), 4096);
    let proof = fixture
        .observe(Ok(khronos_egl::CONDITION_SATISFIED))
        .unwrap()
        .unwrap();
    assert!(proof.is_for(&fixture.allocation, &fixture.generation, &17));
    assert_eq!(fixture.state, Observation::Issued);
    let result = fixture.state.observe(
        &fixture.allocation,
        &fixture.generation,
        Some(17),
        Some(&fixture.guard),
        |_| panic!("issued completion must not poll or issue again"),
    );
    assert!(matches!(result, Err(Refusal::AlreadyIssued)));
    // Observation did not consume or drop the existing native fence's use.
    assert_eq!(fixture.generation.pending_uses.get(), 1);
}

#[test]
fn errors_and_unexpected_statuses_never_become_completion() {
    for result in [Err(()), Ok(0), Ok(khronos_egl::SUCCESS)] {
        let mut fixture = Fixture::new();
        assert!(matches!(fixture.observe(result), Err(Refusal::Uncertain)));
        assert_eq!(fixture.state, Observation::Uncertain);
        assert!(!fixture.generation.reuse_allowed());
        assert_eq!(fixture.generation.pending_uses.get(), 1);
        assert_eq!(fixture.accounting.bytes.get(), 4096);
        let result = fixture.state.observe(
            &fixture.allocation,
            &fixture.generation,
            Some(17),
            Some(&fixture.guard),
            |_| panic!("uncertain observation cannot be retried into a success proof"),
        );
        assert!(matches!(result, Err(Refusal::Uncertain)));
    }
}

#[test]
fn absent_or_already_transferred_fence_cannot_prove_completion() {
    let mut fixture = Fixture::new();
    let called = Cell::new(false);
    let result = fixture.state.observe(
        &fixture.allocation,
        &fixture.generation,
        None::<usize>,
        Some(&fixture.guard),
        |_| {
            called.set(true);
            Ok(khronos_egl::CONDITION_SATISFIED)
        },
    );
    assert!(matches!(result, Err(Refusal::MissingFence)));
    assert!(!called.get());
    assert!(!fixture.generation.reuse_allowed());
    assert_eq!(fixture.generation.pending_uses.get(), 1);
}

#[test]
fn missing_or_different_generation_use_refuses_before_polling() {
    for missing in [false, true] {
        let mut fixture = Fixture::new();
        // Identical numeric IDs are insufficient: this is a different use.
        let other = NativeSnapshotGeneration::new(1, 1);
        let other_guard = other.acquire_use();
        let result = fixture.state.observe(
            &fixture.allocation,
            &fixture.generation,
            Some(17),
            if missing { None } else { Some(&other_guard) },
            |_| panic!("wrong use must not poll"),
        );
        assert!(matches!(result, Err(Refusal::WrongGeneration)));
        assert!(!fixture.generation.reuse_allowed());
        assert!(other.reuse_allowed());
        assert_eq!(fixture.generation.pending_uses.get(), 1);
    }
}

#[test]
fn proof_is_bound_to_allocation_generation_and_exact_sync() {
    let mut fixture = Fixture::new();
    let proof = fixture
        .observe(Ok(khronos_egl::CONDITION_SATISFIED))
        .unwrap()
        .unwrap();
    let other_allocation = NativeCaptureAllocationCharge::new(&fixture.accounting, 4096);
    let other_generation = NativeSnapshotGeneration::new(1, 1);
    assert!(!proof.is_for(&other_allocation, &fixture.generation, &17));
    assert!(!proof.is_for(&fixture.allocation, &other_generation, &17));
    assert!(!proof.is_for(&fixture.allocation, &fixture.generation, &18));
    assert!(proof.is_for(&fixture.allocation, &fixture.generation, &17));
}

#[test]
fn proof_keeps_its_allocation_alive_but_does_not_settle_the_gpu_use() {
    let mut fixture = Fixture::new();
    let proof = fixture
        .observe(Ok(khronos_egl::CONDITION_SATISFIED))
        .unwrap()
        .unwrap();
    let accounting = fixture.accounting.clone();
    let generation = fixture.generation.clone();
    let weak = Rc::downgrade(&fixture.allocation);
    assert_eq!(generation.pending_uses.get(), 1);
    drop(fixture);
    // Its own original guard was dropped by the simulated capture owner, not
    // by polling. The proof keeps allocation identity and accounting alive.
    assert_eq!(generation.pending_uses.get(), 0);
    assert!(weak.upgrade().is_some());
    assert_eq!(accounting.bytes.get(), 4096);
    drop(proof);
    assert!(weak.upgrade().is_none());
    assert_eq!(accounting.bytes.get(), 0);
}

#[test]
fn completed_reads_do_not_restore_permission_to_recycle() {
    let mut fixture = Fixture::new();
    fixture.generation.abandon_reuse();
    fixture.generation.mark_exported();
    assert!(
        fixture
            .observe(Ok(khronos_egl::CONDITION_SATISFIED))
            .unwrap()
            .is_some()
    );
    assert!(!fixture.generation.reuse_allowed());
    assert!(!fixture.generation.can_recycle());
}
