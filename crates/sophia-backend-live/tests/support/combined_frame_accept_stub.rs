//! G3-4: the renderer side of the combined accept fixture. The core's
//! commands come to this test instead of a renderer thread, so the real
//! combined route is served here and its one render is an injected exported
//! frame: the deterministic render boundary. Admission, forwarding, the
//! route, settlement, acceptance, `poll()` and everything after it are
//! production code.

use super::*;
use sophia_renderer_live::LiveRendererScanoutBufferDescriptor;
use std::sync::mpsc::SyncSender;

/// A core whose commands this test reads, and the combined route it serves.
pub(crate) struct StubRenderer {
    commands: Receiver<WorkerCommand>,
    _resume: SyncSender<()>,
    /// The ordinary reply channel of the attached facade, kept open.
    _ordinary: Vec<sophia_wake::SignalSender<WorkerResult>>,
    route: Option<OutputRoute>,
    releases: usize,
}

pub(crate) fn stub_core() -> (Arc<NativeGbmRendererWorkerCore>, StubRenderer) {
    let (core, commands, resume) = super::super::inventory_test_core(WORKER_COMMAND_CAPACITY);
    (
        core,
        StubRenderer {
            commands,
            _resume: resume,
            _ordinary: Vec::new(),
            route: None,
            releases: 0,
        },
    )
}

impl StubRenderer {
    /// Takes the facade's attach registration and then its route
    /// registration, sent on its first combined reservation, for `output`.
    pub(crate) fn register(
        &mut self,
        output: LiveRendererWorkerOutputKey,
        facade: &CombinedFacade,
    ) {
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
        let route = self.route.as_mut().expect("registered before serving");
        let end = Instant::now() + WAIT;
        let mut served = false;
        while !served {
            assert!(Instant::now() < end, "the validator did not forward");
            route.serve(Instant::now(), |plan, pixels, _| {
                served = true;
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
                let mut result = correlated_result(plan.correlation(), outcome);
                result.output = output;
                RenderAttempt::Done(CombinedOutcome::Rendered(result), pixels)
            });
            std::thread::yield_now();
        }
    }

    /// Release commands the facade sent for leases it dropped or gave up.
    pub(crate) fn releases(&mut self) -> usize {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                WorkerCommand::Release { .. } => self.releases += 1,
                WorkerCommand::Register { reply, .. } => self._ordinary.push(reply),
                _ => panic!("the accept fixture sends no render"),
            }
        }
        self.releases
    }
}
