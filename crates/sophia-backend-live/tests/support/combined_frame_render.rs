//! Stage 4c-2: the renderer's combined inbox, parking, priority and deadline,
//! with real validator-forwarded frames and no GPU.

use super::super::combined::{
    CombinedOutcome, CombinedRenderPlan, CombinedRenderResult, OutputRoute,
};
use super::super::{
    DeviceIdentity, LiveRendererWorkerOutputKey, NEXT_IMAGE_STORE, NativeGbmRendererWorker,
    NativeGbmRendererWorkerCore, WorkerCommand, WorkerControl, WorkerOutcome, WorkerRegistry,
    WorkerResult, combined::COMBINED_REQUEST, frame_correlation, wrap_commands,
};
use super::{correlated_facade, correlated_result, mixed_job, run_worker};
use crate::api::LiveGbmEglFrameTargetRecord;
use sophia_protocol::Size;
use sophia_renderer_live::LiveRendererScanoutBufferExportDetail;
use sophia_runtime::{CombinedReservation, ForwardedValidation, IsbValidationWorker};
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
    let reservation = validator
        .reserve_combined(8, Instant::now() + WAIT)
        .unwrap();
    let inbox = facade.combined_inbox().expect("facade takes combined work");
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
    route.serve(Instant::now(), Err);
    assert!(route.deadline().is_some(), "no slot parks the frame");
    let second = forward(&mut validator, &mut facade, 2, later);

    let mut served = None;
    route.serve(Instant::now(), |plan| {
        served = generation(&plan);
        Ok(rendered(1))
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
        Ok(rendered(2))
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
    route.serve(Instant::now(), Err);
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
    route.serve(Instant::now(), |_| Ok(rendered(5)));
    let _second = forward(&mut validator, &mut facade, 6, later);
    route.serve(Instant::now(), |_| Ok(rendered(6)));
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
