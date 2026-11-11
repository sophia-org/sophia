//! Stage 4c-2: the renderer's combined inbox, parking, priority and deadline,
//! with real validator-forwarded frames and no GPU. Stage 4c-3: the facade's
//! reserve, cancel and settle calls, which keep the Release cell and the
//! validation credit in one order.

use super::super::combined::{
    CombinedOutcome, CombinedRenderPlan, CombinedRenderResult, OutputRoute, RenderAttempt,
};
use super::super::{
    CombinedReleaseCell, DeviceIdentity, LiveRendererWorkerLeaseId, LiveRendererWorkerOutputKey,
    NEXT_IMAGE_STORE, NativeGbmRendererWorker, NativeGbmRendererWorkerCore, WorkerCommand,
    WorkerControl, WorkerOutcome, WorkerRegistry, WorkerResult, combined::COMBINED_REQUEST,
    frame_correlation, wrap_commands,
};
use super::{correlated_facade, correlated_result, exported_outcome, mixed_job, run_worker};
use crate::api::LiveGbmEglFrameTargetRecord;
use sophia_protocol::Size;
use sophia_renderer_live::LiveRendererScanoutBufferExportDetail;
use sophia_runtime::{CombinedInbox, CombinedReservation, IsbValidationWorker};
use std::io::Write;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::time::{Duration, Instant};

type Validator = IsbValidationWorker<CombinedRenderPlan>;
const WAIT: Duration = Duration::from_secs(10);

fn pixels() -> OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let mut file = std::fs::File::from(
        memfd_create(
            "combined-render",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.write_all(&[0; 8]).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    file.into()
}

fn plan(generation: u64, deadline: Instant) -> CombinedRenderPlan {
    CombinedRenderPlan::new(
        deadline,
        LiveGbmEglFrameTargetRecord::new(Size {
            width: 16,
            height: 16,
        }),
        mixed_job(generation),
        Vec::new(),
        None,
    )
}

fn generation(plan: &CombinedRenderPlan) -> Option<u64> {
    match &plan.frame {
        super::super::PendingRenderedFrame::Mixed(frame, _) => {
            frame.trace.as_ref().map(|trace| trace.scene_generation)
        }
        _ => None,
    }
}

/// Validates one frame and forwards it into the facade's real inbox.
fn forward(
    validator: &mut Validator,
    facade: &mut NativeGbmRendererWorker,
    generation: u64,
    deadline: Instant,
) -> CombinedReservation {
    let inbox = facade.combined_inbox().expect("facade takes combined work");
    submit(validator, inbox, generation, deadline)
}

/// Validates one frame into an inbox the caller already holds.
fn submit(
    validator: &mut Validator,
    inbox: CombinedInbox<CombinedRenderPlan>,
    generation: u64,
    deadline: Instant,
) -> CombinedReservation {
    let reservation = validator
        .reserve_combined(8, Instant::now() + WAIT)
        .unwrap();
    let _owner_plan = validator
        .submit_combined(&reservation, pixels(), plan(generation, deadline), inbox)
        .ok()
        .expect("combined submission");
    let end = Instant::now() + WAIT;
    while !reservation.was_forwarded() {
        assert!(Instant::now() < end, "the validator did not forward");
        std::thread::yield_now();
    }
    reservation
}

/// The worker's half of a facade's registration, taken off a test channel.
fn route(facade: &mut NativeGbmRendererWorker, commands: &Receiver<WorkerCommand>) -> OutputRoute {
    let _ = facade.combined_inbox().expect("facade takes combined work");
    let WorkerCommand::RegisterCombined(registration) = commands.recv().unwrap() else {
        panic!("first combined use registers the route");
    };
    registration
        .route_for(facade.output(), &facade.claim)
        .expect("route for the live claim")
}

fn rendered(generation: u64) -> WorkerResult {
    correlated_result(
        frame_correlation(&mixed_job(generation), Some(COMBINED_REQUEST)),
        WorkerOutcome::Failed(LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable),
    )
}

fn settle(
    validator: &mut Validator,
    reservation: CombinedReservation,
    result: CombinedRenderResult,
) {
    assert_eq!(
        result.pixels.bytes(),
        &[0; 8],
        "the validated pixels come home intact"
    );
    assert!(
        validator
            .settle_combined_token(reservation, result.token)
            .is_ok(),
        "the returned token settles its own reservation"
    );
}

#[test]
fn a_parked_frame_takes_the_next_slot_before_the_inbox_is_read() {
    let (mut facade, commands, _reply) = correlated_facade();
    let mut route = route(&mut facade, &commands);
    let mut validator = Validator::for_combined_plans(2, 16).unwrap();
    let later = Instant::now() + WAIT;
    let first = forward(&mut validator, &mut facade, 1, later);
    route.serve(Instant::now(), RenderAttempt::NoSlot);
    assert!(route.deadline().is_some(), "no slot parks the frame");
    let second = forward(&mut validator, &mut facade, 2, later);

    let mut served = None;
    route.serve(Instant::now(), |plan| {
        served = generation(&plan);
        RenderAttempt::Rendered(rendered(1))
    });
    assert_eq!(
        served,
        Some(1),
        "the parked frame is served before the inbox"
    );
    assert!(route.deadline().is_none());
    settle(
        &mut validator,
        first,
        facade.take_combined_result().unwrap(),
    );

    let mut served = None;
    route.serve(Instant::now(), |plan| {
        served = generation(&plan);
        RenderAttempt::Rendered(rendered(2))
    });
    assert_eq!(served, Some(2), "the inbox frame waited for the parked one");
    settle(
        &mut validator,
        second,
        facade.take_combined_result().unwrap(),
    );
}

#[test]
fn expiry_returns_token_and_pixels_from_the_inbox_and_from_parking() {
    let (mut facade, commands, _reply) = correlated_facade();
    let mut route = route(&mut facade, &commands);
    let mut validator = Validator::for_combined_plans(2, 16).unwrap();

    let expired = forward(&mut validator, &mut facade, 3, Instant::now());
    route.serve(Instant::now(), |_| {
        panic!("an expired frame is never rendered")
    });
    let result = facade.take_combined_result().unwrap();
    assert!(
        matches!(result.outcome, CombinedOutcome::Expired),
        "expired at dequeue"
    );
    settle(&mut validator, expired, result);

    let deadline = Instant::now() + WAIT;
    let parked = forward(&mut validator, &mut facade, 4, deadline);
    route.serve(Instant::now(), RenderAttempt::NoSlot);
    assert_eq!(route.deadline(), Some(deadline));
    route.serve(deadline, |_| panic!("an expired frame is never rendered"));
    let result = facade.take_combined_result().unwrap();
    assert!(
        matches!(result.outcome, CombinedOutcome::Expired),
        "expired while parked"
    );
    assert!(route.deadline().is_none());
    settle(&mut validator, parked, result);
}

#[test]
fn a_full_result_route_is_counted_and_never_blocks() {
    let (mut facade, commands, _reply) = correlated_facade();
    let mut route = route(&mut facade, &commands);
    let mut validator = Validator::for_combined_plans(2, 16).unwrap();
    let later = Instant::now() + WAIT;
    let _first = forward(&mut validator, &mut facade, 5, later);
    route.serve(Instant::now(), |_| RenderAttempt::Rendered(rendered(5)));
    let _second = forward(&mut validator, &mut facade, 6, later);
    route.serve(Instant::now(), |_| RenderAttempt::Rendered(rendered(6)));
    assert_eq!(
        route.undelivered, 1,
        "a second result for one output is a broken contract"
    );
}

/// A core whose worker thread runs the real loop without a render device.
fn running_core(inode: u64) -> (Arc<NativeGbmRendererWorkerCore>, Arc<WorkerControl>) {
    let registry = Arc::new(WorkerRegistry::default());
    let identity = DeviceIdentity {
        device: 1,
        inode,
        special_device: 226 << 8,
    };
    let control = Arc::new(WorkerControl::default());
    let service_control = Arc::clone(&control);
    let (commands, receive) = sync_channel(32);
    let thread = registry
        .reserve(identity)
        .unwrap()
        .start(|| {
            std::thread::Builder::new().spawn(move || {
                run_worker::<std::fs::File>(
                    Err(std::io::Error::other("no render device in this test")),
                    Vec::new(),
                    receive,
                    service_control,
                );
            })
        })
        .unwrap();
    let core = Arc::new(NativeGbmRendererWorkerCore {
        image_store: NEXT_IMAGE_STORE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        command_sender: wrap_commands(commands, &control),
        _thread: std::sync::Mutex::new(thread),
        control: Arc::clone(&control),
        inventory_replacement: std::sync::Mutex::new(None),
        #[cfg(feature = "experimental-shell-capture")]
        capture_install: std::sync::Mutex::new(None),
        #[cfg(feature = "experimental-shell-capture")]
        capture_prepare: std::sync::Mutex::new(None),
        release_enqueue_failures: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    });
    (core, control)
}

fn result_after_wake(
    facade: &NativeGbmRendererWorker,
    owner: &sophia_wake::Wake,
) -> CombinedRenderResult {
    let end = Instant::now() + WAIT;
    loop {
        match facade.take_combined_result() {
            Ok(result) => return result,
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => panic!("combined route closed"),
        }
        assert!(
            owner.wait(Some(end)).unwrap(),
            "a forwarded frame reaches an idle renderer and its result rings the owner"
        );
        owner.clear().unwrap();
    }
}

#[test]
fn a_frame_forwarded_to_an_idle_renderer_comes_home_and_rings_the_owner() {
    let (core, _control) = running_core(9_401);
    let mut facade = core.attach(LiveRendererWorkerOutputKey::from_raw(941));
    let owner = sophia_wake::Wake::new().unwrap();
    facade.attach_owner_wake(&owner.notifier());
    let _ = facade.combined_inbox().expect("facade takes combined work");
    // Lets the renderer finish the registration and reach its wait; without
    // the pause the test still passes but proves less about the wake.
    std::thread::sleep(Duration::from_millis(200));
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let reservation = forward(&mut validator, &mut facade, 7, Instant::now() + WAIT);
    let result = result_after_wake(&facade, &owner);
    assert!(matches!(
        &result.outcome,
        CombinedOutcome::Rendered(WorkerResult {
            outcome: WorkerOutcome::Failed(
                LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable
            ),
            ..
        })
    ));
    settle(&mut validator, reservation, result);
}

#[test]
fn an_expired_frame_comes_home_expired_from_the_running_renderer() {
    let (core, _control) = running_core(9_402);
    let mut facade = core.attach(LiveRendererWorkerOutputKey::from_raw(942));
    let owner = sophia_wake::Wake::new().unwrap();
    facade.attach_owner_wake(&owner.notifier());
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let reservation = forward(&mut validator, &mut facade, 8, Instant::now());
    let result = result_after_wake(&facade, &owner);
    assert!(matches!(result.outcome, CombinedOutcome::Expired));
    settle(&mut validator, reservation, result);
}

#[test]
fn shutdown_ends_a_renderer_waiting_on_its_command_wake() {
    let (core, control) = running_core(9_403);
    let mut facade = core.attach(LiveRendererWorkerOutputKey::from_raw(943));
    // A combined route moves the renderer from recv() onto its command wake.
    let _ = facade.combined_inbox().expect("facade takes combined work");
    std::thread::sleep(Duration::from_millis(200));
    control.shutdown();
    let end = Instant::now() + WAIT;
    let mut thread = core._thread.lock().unwrap();
    while !thread.poll_join().unwrap() {
        assert!(Instant::now() < end, "shutdown must end a waiting renderer");
        std::thread::yield_now();
    }
}

fn exported(generation: u64, lease: u64) -> WorkerResult {
    let mut outcome = exported_outcome();
    let WorkerOutcome::Exported { lease_id, .. } = &mut outcome else {
        unreachable!();
    };
    *lease_id = LiveRendererWorkerLeaseId(lease);
    correlated_result(
        frame_correlation(&mixed_job(generation), Some(COMBINED_REQUEST)),
        outcome,
    )
}

/// Reserves as the owner will, then renders one exported frame through the
/// worker's half of the route the reservation registered.
fn exported_result(
    facade: &mut NativeGbmRendererWorker,
    commands: &Receiver<WorkerCommand>,
    validator: &mut Validator,
    lease: u64,
) -> (CombinedReservation, CombinedRenderResult) {
    let inbox = facade
        .reserve_combined_frame()
        .expect("an idle facade reserves");
    let WorkerCommand::RegisterCombined(registration) = commands.recv().unwrap() else {
        panic!("first combined use registers the route");
    };
    let mut route = registration
        .route_for(facade.output(), &facade.claim)
        .expect("route for the live claim");
    let reservation = submit(validator, inbox, lease, Instant::now() + WAIT);
    route.serve(Instant::now(), |_| {
        RenderAttempt::Rendered(exported(lease, lease))
    });
    (reservation, facade.take_combined_result().unwrap())
}

fn assert_retained(facade: &NativeGbmRendererWorker, lease: u64, why: &str) {
    assert!(
        matches!(&facade.combined_release, CombinedReleaseCell::Retained(release)
            if release.lease_id == LiveRendererWorkerLeaseId(lease)),
        "{why}"
    );
}

#[test]
fn a_reservation_holds_the_release_cell_until_cancelled() {
    let (mut facade, commands, _reply) = correlated_facade();
    assert!(facade.reserve_combined_frame().is_some());
    assert!(matches!(
        commands.try_recv(),
        Ok(WorkerCommand::RegisterCombined(_))
    ));
    assert!(
        facade.reserve_combined_frame().is_none(),
        "one combined job per facade"
    );
    facade.cancel_combined_frame();
    assert!(
        facade.reserve_combined_frame().is_some(),
        "a cancelled reservation frees the cell"
    );
    assert!(commands.try_recv().is_err(), "the route registers once");
}

#[test]
fn a_running_renderers_result_settles_its_credit_and_frees_the_cell() {
    let (core, _control) = running_core(9_404);
    let mut facade = core.attach(LiveRendererWorkerOutputKey::from_raw(944));
    let owner = sophia_wake::Wake::new().unwrap();
    facade.attach_owner_wake(&owner.notifier());
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let inbox = facade
        .reserve_combined_frame()
        .expect("an idle facade reserves");
    let reservation = submit(&mut validator, inbox, 10, Instant::now() + WAIT);
    let result = result_after_wake(&facade, &owner);
    let pixels = facade
        .settle_combined_frame(&mut validator, reservation, result)
        .ok()
        .expect("the frame's own reservation settles");
    assert_eq!(pixels.bytes(), &[0; 8], "the validated pixels come home");
    assert_eq!(validator.pending(), 0, "settlement clears the credit");
    assert!(
        facade.reserve_combined_frame().is_some(),
        "nothing was exported, so the cell is Free"
    );
}

#[test]
fn an_exported_result_keeps_its_lease_in_the_cell_until_the_next_poll() {
    let (mut facade, commands, _reply) = correlated_facade();
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let (reservation, result) = exported_result(&mut facade, &commands, &mut validator, 51);
    let pixels = facade
        .settle_combined_frame(&mut validator, reservation, result)
        .ok()
        .expect("the frame's own reservation settles");
    assert_eq!(pixels.bytes(), &[0; 8], "the validated pixels come home");
    assert_eq!(validator.pending(), 0, "settlement clears the credit");
    assert_retained(&facade, 51, "the lease has an owner once the credit clears");
    assert!(
        facade.reserve_combined_frame().is_none(),
        "a retained lease holds the cell"
    );
    facade.cancel_combined_frame();
    assert_retained(&facade, 51, "a stray cancel cannot drop a retained lease");
    let _ = facade.poll();
    assert!(
        matches!(commands.try_recv(), Ok(WorkerCommand::Release { lease_id, .. })
            if lease_id == LiveRendererWorkerLeaseId(51)),
        "the next poll sends the Release"
    );
    assert!(facade.reserve_combined_frame().is_some());
}

#[test]
fn a_foreign_reservation_is_refused_with_everything_returned() {
    let (mut facade, commands, _reply) = correlated_facade();
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let (reservation, result) = exported_result(&mut facade, &commands, &mut validator, 52);
    let mut stranger = Validator::for_combined_plans(1, 8).unwrap();
    let foreign = stranger.reserve_combined(8, Instant::now() + WAIT).unwrap();
    let Err((_foreign, token, pixels)) =
        facade.settle_combined_frame(&mut validator, foreign, result)
    else {
        panic!("a foreign reservation must not settle");
    };
    assert_eq!(pixels.bytes(), &[0; 8], "refusal returns the pixels");
    assert_eq!(validator.pending(), 1, "refusal keeps the credit");
    assert_retained(&facade, 52, "refusal keeps the lease owned");
    assert!(
        validator.settle_combined_token(reservation, token).is_ok(),
        "the returned token still settles its own reservation"
    );
}
