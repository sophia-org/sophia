//! G3-4: the renderer side of the combined accept fixture. The core's
//! commands come to this test instead of a renderer thread, so the real
//! combined route is served here and its one render is an injected exported
//! frame: the deterministic render boundary. Admission, forwarding, the
//! route, settlement, acceptance, `poll()` and everything after it are
//! production code.
//!
//! Mounted beside the worker for this crate's tests and, under test-support,
//! for the public combined fixture, which reaches only `stub_core` until
//! G3-5d's running renderer.

use super::combined::{CombinedOutcome, OutputRoute, RenderAttempt};
use super::service::{
    CombinedGate, CombinedRenderSeam, GateHandle, combined_gate, pausing_combined_gate,
};
use super::support::{correlated_result, exported_outcome, inventory_test_core};
use super::{
    CombinedFacade, DeviceIdentity, LiveRendererWorkerLeaseId, LiveRendererWorkerOutputKey,
    NEXT_IMAGE_STORE, NativeGbmRendererWorkerCore, WORKER_COMMAND_CAPACITY, WorkerCommand,
    WorkerControl, WorkerOutcome, WorkerRegistry, WorkerResult, run_worker, wrap_commands,
};
use sophia_protocol::Size;
use sophia_renderer_live::LiveRendererScanoutBufferDescriptor;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);

/// What the injected render does with a frame the route lets it render.
#[derive(Clone, Copy, Debug)]
pub enum StubReply {
    Exported {
        size: Size,
        lease: u64,
    },
    /// The render ran and did not export.
    Failed,
    /// The renderer found the pixels disagree with the admitted description.
    Mismatch,
}

/// A core whose commands this test reads, and the combined route it serves.
pub(crate) struct StubRenderer {
    commands: Receiver<WorkerCommand>,
    _resume: SyncSender<()>,
    /// The ordinary reply channel of the attached facade, kept open.
    _ordinary: Vec<sophia_wake::SignalSender<WorkerResult>>,
    route: Option<OutputRoute>,
    releases: usize,
    /// The core's command sender, for `fill`.
    commands_in: super::CommandSender,
    /// Filler commands `fill` queued and not yet drained.
    filler: usize,
}

pub(crate) fn stub_core() -> (Arc<NativeGbmRendererWorkerCore>, StubRenderer) {
    let (core, commands, resume) = inventory_test_core(WORKER_COMMAND_CAPACITY);
    let commands_in = core.command_sender.clone();
    (
        core,
        StubRenderer {
            commands,
            _resume: resume,
            _ordinary: Vec::new(),
            route: None,
            releases: 0,
            commands_in,
            filler: 0,
        },
    )
}

/// G3-5d d-2b2: the renderer gate (d-2a) armed on a running core, for the
/// running fixture's test-support consumers. Only its facts, release and
/// disconnect cross; the worker half stays in the worker.
#[doc(hidden)]
pub struct CombinedRenderGate(GateHandle);

pub use super::service::{GateFact as CombinedGateFact, GateRelease as CombinedGateRelease};

impl CombinedRenderGate {
    /// The worker's next fact, or None after `wait`.
    pub fn next(&self, wait: Duration) -> Option<CombinedGateFact> {
        self.0.next(wait)
    }

    /// Ends the hold as `Released`, once.
    pub fn release(&mut self) {
        self.0.release();
    }

    /// Drops the release side only: a hold ends as `Disconnected`, observed.
    pub fn disconnect(&mut self) {
        self.0.disconnect();
    }

    /// Ends a pausing gate's one pause, once (gate 4 C3a, the running
    /// fixture's loss-and-resume operation).
    pub(crate) fn resume(&mut self) {
        self.0.resume();
    }
}

/// Arms the worker's gate for `output` and waits for its acknowledgement,
/// so nothing admitted afterwards passes the gate unseen. None if the worker
/// did not take the gate or did not acknowledge it within `WAIT`.
pub(crate) fn arm_render_gate(
    core: &NativeGbmRendererWorkerCore,
    output: LiveRendererWorkerOutputKey,
    wait: Duration,
) -> Option<CombinedRenderGate> {
    arm(core, combined_gate(output, wait))
}

/// `arm_render_gate` with the pausing gate (gate 4 C3a): it also pauses
/// once, bounded by `wait`, the first time it finds its output's route
/// empty, and serves nothing in that pass after the pause ends.
pub(crate) fn arm_pausing_render_gate(
    core: &NativeGbmRendererWorkerCore,
    output: LiveRendererWorkerOutputKey,
    wait: Duration,
) -> Option<CombinedRenderGate> {
    arm(core, pausing_combined_gate(output, wait))
}

fn arm(
    core: &NativeGbmRendererWorkerCore,
    (gate, handle): (CombinedGate, GateHandle),
) -> Option<CombinedRenderGate> {
    core.command_sender
        .send(WorkerCommand::InstallCombinedGate(gate))
        .ok()?;
    (handle.next(WAIT) == Some(CombinedGateFact::Armed)).then_some(CombinedRenderGate(handle))
}

/// `running_core` with its seam, for the running combined fixture, which
/// never reads the control.
pub(crate) fn running_seam_core(
    inode: u64,
    seam: CombinedRenderSeam,
) -> Arc<NativeGbmRendererWorkerCore> {
    running_core(inode, Some(seam)).0
}

/// A core whose worker thread runs the real loop without a render device;
/// with `seam`, its bound combined frames render through that deterministic
/// seam (G3-5d d-1) before any other command is served.
pub(super) fn running_core(
    inode: u64,
    seam: Option<CombinedRenderSeam>,
) -> (Arc<NativeGbmRendererWorkerCore>, Arc<WorkerControl>) {
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
    if let Some(seam) = seam {
        assert!(
            core.command_sender
                .send(WorkerCommand::InstallRenderSeam(seam))
                .is_ok(),
            "the running worker takes its seam first"
        );
    }
    (core, control)
}

// Until a test-support consumer drives the renderer (G3-5d), only this
// crate's tests call these.
#[cfg_attr(not(test), allow(dead_code))]
impl StubRenderer {
    /// Takes the facade's attach registration and then its route
    /// registration, sent on its first combined reservation, for `output`.
    /// One route per stub: once registered, this does nothing.
    pub(crate) fn register(
        &mut self,
        output: LiveRendererWorkerOutputKey,
        facade: &CombinedFacade,
    ) {
        if self.route.is_some() {
            return;
        }
        loop {
            match self.commands.recv_timeout(WAIT).expect("a registration") {
                WorkerCommand::Register { reply, .. } => self._ordinary.push(reply),
                WorkerCommand::RegisterCombined(registration) => {
                    self.route = Some(
                        registration
                            .route_for(output, facade.claim())
                            .expect("route for the issuing facade's claim"),
                    );
                    return;
                }
                WorkerCommand::Release { .. } => self.releases += 1,
                _ => panic!("the accept fixture sends no render"),
            }
        }
    }

    /// Serves the one forwarded frame: an exported render of `size` as lease
    /// `lease`, carrying the plan's own correlation, on `output`.
    pub(crate) fn serve_exported(
        &mut self,
        output: LiveRendererWorkerOutputKey,
        size: Size,
        lease: u64,
    ) {
        self.serve(output, Instant::now(), StubReply::Exported { size, lease });
    }

    /// Serves the one forwarded frame at `at` with `reply`. The real route
    /// decides Stopped (the owner stopped the job) and Expired (`at` at or
    /// past the plan's deadline) without rendering; otherwise `reply` is the
    /// injected render.
    pub(crate) fn serve(
        &mut self,
        output: LiveRendererWorkerOutputKey,
        at: Instant,
        reply: StubReply,
    ) {
        let route = self.route.as_mut().expect("registered before serving");
        assert!(route.await_job(WAIT), "the validator did not forward");
        route.serve(at, |plan, pixels, _| {
            let outcome = match reply {
                StubReply::Exported { size, lease } => {
                    let mut outcome = exported_outcome();
                    let WorkerOutcome::Exported {
                        descriptor,
                        lease_id,
                        ..
                    } = &mut outcome
                    else {
                        unreachable!();
                    };
                    *descriptor = LiveRendererScanoutBufferDescriptor::new(
                        size,
                        u32::try_from(size.width).unwrap() * 4,
                        sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                        1,
                    );
                    *lease_id = LiveRendererWorkerLeaseId(lease);
                    outcome
                }
                StubReply::Failed => WorkerOutcome::Failed(
                    sophia_renderer_live::LiveRendererScanoutBufferExportDetail::InvalidTarget,
                ),
                StubReply::Mismatch => {
                    return RenderAttempt::Done(CombinedOutcome::Mismatch, pixels);
                }
            };
            let mut result = correlated_result(plan.correlation(), outcome);
            result.output = output;
            RenderAttempt::Done(CombinedOutcome::Rendered(result), pixels)
        });
        assert!(route.deadline().is_none(), "the stub never parks a frame");
    }

    /// Parks the one forwarded frame until `serve` decides it, so the owner
    /// can stop the job between its forwarding and its render.
    pub(crate) fn park(&mut self) -> bool {
        self.route
            .as_mut()
            .expect("registered before parking")
            .await_job(WAIT)
    }

    /// Fills the command queue so the next Release enqueue sees `Full`. The
    /// queue holds only filler until the next `releases`.
    pub(crate) fn fill(&mut self) {
        self.releases();
        loop {
            match self.commands_in.try_send(WorkerCommand::Shutdown) {
                Ok(()) => self.filler += 1,
                Err(TrySendError::Full(_)) => return,
                Err(TrySendError::Disconnected(_)) => panic!("the stub owns the receiver"),
            }
        }
    }

    /// Release commands the facade sent for leases it dropped or gave up.
    /// Draining also takes `fill`'s filler, and only that many Shutdowns.
    pub(crate) fn releases(&mut self) -> usize {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                WorkerCommand::Release { .. } => self.releases += 1,
                WorkerCommand::Register { reply, .. } => self._ordinary.push(reply),
                WorkerCommand::Shutdown if self.filler > 0 => self.filler -= 1,
                _ => panic!("the accept fixture sends no render"),
            }
        }
        self.releases
    }
}
