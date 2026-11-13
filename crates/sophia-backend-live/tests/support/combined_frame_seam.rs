//! G3-5d d-1: the deterministic seam on a running worker, against the
//! worker's own slot pool, damage history, lease ids and lease table. Each
//! control forwards a real validated frame through a facade's reservation
//! and reads the worker's published slot metrics, or what the worker handed
//! the seam; Release commands are the worker's own.

use super::*;
use crate::LiveRendererFrameSlotMetrics;
use sophia_renderer_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888, LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
    LiveCompositionFormatRequest, LiveRendererScanoutBufferDescriptor,
};
use std::sync::atomic::{AtomicUsize, Ordering};

/// What the seam was handed: its call count, and per call whether the
/// worker's repaint table showed the slot with a history at age 1 (`None`
/// when the worker handed no table).
#[derive(Default)]
struct Probe {
    calls: AtomicUsize,
    history: std::sync::Mutex<Vec<Option<bool>>>,
}

/// A running core whose seam answers an XRGB8888 descriptor of the target,
/// or ComposeRefused for the zero-based calls listed in `refuse`.
fn seam_core(
    inode: u64,
    refuse: &'static [usize],
) -> (
    Arc<NativeGbmRendererWorkerCore>,
    Arc<super::super::super::WorkerControl>,
    Arc<Probe>,
) {
    let probe = Arc::new(Probe::default());
    let seen = Arc::clone(&probe);
    let (core, control) = running_core(
        inode,
        Some(Box::new(
            move |_: &crate::PendingRenderedFrame,
                  target: LiveGbmEglFrameTargetRecord,
                  repaint: Option<&sophia_renderer_live::NativeCompositionRepaintTable>| {
                let call = seen.calls.fetch_add(1, Ordering::SeqCst);
                seen.history.lock().unwrap().push(repaint.map(|table| {
                    table.full_reason_for_age(1)
                        != Some(sophia_renderer_live::NativeFullRepaintReason::NoHistory)
                }));
                if refuse.contains(&call) {
                    return Err(LiveRendererScanoutBufferExportDetail::ComposeRefused);
                }
                let pitch = u32::try_from(target.size.width)
                    .map_err(|_| LiveRendererScanoutBufferExportDetail::InvalidTarget)?
                    * 4;
                Ok(LiveRendererScanoutBufferDescriptor::new(
                    target.size,
                    pitch,
                    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                    1,
                ))
            },
        )),
    );
    (core, control, probe)
}

struct Output {
    facade: NativeGbmRendererWorker,
    owner: sophia_wake::Wake,
    validator: Validator,
}

impl Output {
    fn attach(core: &Arc<NativeGbmRendererWorkerCore>, key: u64) -> Self {
        let facade = core.attach(LiveRendererWorkerOutputKey::from_raw(key));
        let owner = sophia_wake::Wake::new().unwrap();
        facade.attach_owner_wake(&owner.notifier());
        Self {
            facade,
            owner,
            validator: Validator::for_combined_plans(1, 8).unwrap(),
        }
    }

    /// One frame through reserve, validation, the seam and settlement.
    /// Answers its job and the worker's export or failure; the cell is Held
    /// when it exported.
    fn render(
        &mut self,
        generation: u64,
        format: Option<LiveCompositionFormatRequest>,
    ) -> (
        CombinedFacade,
        u64,
        Result<Lease, LiveRendererScanoutBufferExportDetail>,
    ) {
        let mut plan = plan(generation, Instant::now() + WAIT);
        plan.output_format = format;
        self.render_plan(plan)
    }

    /// `render` for a plan the caller built.
    fn render_plan(
        &mut self,
        plan: CombinedRenderPlan,
    ) -> (
        CombinedFacade,
        u64,
        Result<Lease, LiveRendererScanoutBufferExportDetail>,
    ) {
        let (issued, inbox) = self
            .facade
            .reserve_combined_frame()
            .expect("an idle facade reserves");
        let reservation = self
            .validator
            .reserve_combined(8, Instant::now() + WAIT)
            .unwrap();
        let _owner_plan = self
            .validator
            .submit_combined(&reservation, pixels(), plan, inbox)
            .ok()
            .expect("combined submission");
        let job = reservation.job();
        let result = result_after_wake(&self.facade, &self.owner);
        let CombinedOutcome::Rendered(rendered) = &result.outcome else {
            panic!("the seam's frame is rendered, not expired or mismatched");
        };
        let outcome = exported(&rendered.outcome);
        assert!(
            self.facade
                .settle_combined_frame(&issued, &mut self.validator, reservation, result)
                .is_ok(),
            "the frame's own reservation settles"
        );
        (issued, job, outcome)
    }

    fn slots(&self) -> LiveRendererFrameSlotMetrics {
        self.facade.metrics().frame_slots
    }

    /// Waits until the worker has published `done`; the worker serves
    /// commands in order, so every earlier command was served too.
    fn until(&self, why: &str, done: impl Fn(&LiveRendererFrameSlotMetrics) -> bool) {
        let end = Instant::now() + WAIT;
        while !done(&self.slots()) {
            assert!(Instant::now() < end, "{why}: {:?}", self.slots());
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn send(&self, output: LiveRendererWorkerOutputKey, lease: Lease) {
        self.facade
            .core
            .command_sender
            .try_send(WorkerCommand::Release {
                output,
                lease_id: lease.0,
                slot_token: lease.1,
            })
            .expect("the command queue takes a Release");
    }
}

type Lease = (LiveRendererWorkerLeaseId, crate::LiveRendererFrameSlotToken);

/// The worker's export, checked for the seam's descriptor and no capture,
/// or its failure.
fn exported(outcome: &WorkerOutcome) -> Result<Lease, LiveRendererScanoutBufferExportDetail> {
    match outcome {
        WorkerOutcome::Exported {
            capture,
            descriptor,
            lease_id,
            slot_token,
        } => {
            assert!(capture.is_none(), "the seam constructs no capture");
            assert_eq!(descriptor.format, LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888);
            Ok((*lease_id, *slot_token))
        }
        WorkerOutcome::Failed(detail) => Err(*detail),
        _ => panic!("the seam's frame exports or fails"),
    }
}

#[test]
fn a_seam_render_leases_its_slot_and_its_exact_release_frees_it_once() {
    let (core, _control, probe) = seam_core(9_510, &[]);
    let mut output = Output::attach(&core, 951);
    let (_issued, _job, outcome) = output.render(11, None);
    let lease = outcome.expect("the seam's frame exports");
    assert_eq!(
        lease.0,
        LiveRendererWorkerLeaseId(1),
        "the worker's first lease id"
    );
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1, "one render");
    let slots = output.slots();
    assert_eq!(
        (slots.acquisitions, slots.leased, slots.stale_releases),
        (1, 1, 0),
        "the lease holds one slot"
    );
    output.send(output.facade.output(), lease);
    output.until("the exact Release frees the slot", |slots| {
        slots.leased == 0
    });
    assert_eq!(output.slots().stale_releases, 0);
    output.send(output.facade.output(), lease);
    output.until("a duplicate Release is stale", |slots| {
        slots.stale_releases == 1
    });
    assert_eq!(output.slots().leased, 0);
}

#[test]
fn a_wrong_token_or_another_outputs_release_leaves_the_live_lease() {
    let (core, _control, _probe) = seam_core(9_511, &[]);
    let mut output = Output::attach(&core, 952);
    let other = Output::attach(&core, 953);
    let (issued, job, outcome) = output.render(12, None);
    let first = outcome.expect("the seam's frame exports");
    assert_eq!(
        output.facade.release_combined_held(&issued, job),
        CombinedRelease::Transferred
    );
    output.until("the held result's Release frees the slot", |slots| {
        slots.leased == 0
    });
    let (issued, job, outcome) = output.render(13, None);
    let live = outcome.expect("the seam's frame exports");
    assert_ne!(first.1, live.1, "the slot's token changed");
    assert_ne!(first.0, live.0, "lease ids are never reused");
    let key = output.facade.output();

    output.send(key, (live.0, first.1));
    output.until("a stale token is refused", |slots| {
        slots.stale_releases == 1
    });
    assert_eq!(
        output.slots().leased,
        1,
        "the stale token left the live lease"
    );
    output.send(key, (first.0, live.1));
    output.until("a spent lease id is refused", |slots| {
        slots.stale_releases == 2
    });
    assert_eq!(output.slots().leased, 1, "the spent id left the live lease");
    other.send(other.facade.output(), live);
    other.until("another output refuses the lease", |slots| {
        slots.stale_releases == 1
    });
    assert_eq!(
        output.slots().leased,
        1,
        "another output cannot free this lease"
    );
    output.send(LiveRendererWorkerOutputKey::from_raw(959), live);
    assert_eq!(
        output.facade.release_combined_held(&issued, job),
        CombinedRelease::Transferred
    );
    output.until(
        "the exact Release after a missing output frees the slot",
        |slots| slots.leased == 0,
    );
    assert_eq!(
        (output.slots().stale_releases, other.slots().stale_releases),
        (2, 1),
        "a Release for a missing output counts nothing"
    );
}

#[test]
fn a_refused_render_or_format_frees_its_slot_and_spends_no_lease_id() {
    let (core, _control, probe) = seam_core(9_512, &[0]);
    let mut output = Output::attach(&core, 954);
    let (_issued, _job, outcome) = output.render(14, None);
    assert_eq!(
        outcome,
        Err(LiveRendererScanoutBufferExportDetail::ComposeRefused)
    );
    assert_eq!(output.slots().leased, 0, "a refused render frees its slot");
    let (_issued, _job, outcome) = output.render(
        15,
        Some(LiveCompositionFormatRequest::Required(
            LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
        )),
    );
    assert_eq!(
        outcome,
        Err(LiveRendererScanoutBufferExportDetail::InvalidTarget)
    );
    assert_eq!(output.slots().leased, 0, "a refused format frees its slot");
    let (_issued, _job, outcome) = output.render(16, None);
    assert_eq!(
        outcome.map(|lease| lease.0),
        Ok(LiveRendererWorkerLeaseId(1)),
        "neither failure spent a lease id"
    );
    assert_eq!(probe.calls.load(Ordering::SeqCst), 3);
    let slots = output.slots();
    assert_eq!((slots.acquisitions, slots.leased), (3, 1));
}

#[test]
fn shutdown_ends_a_worker_holding_a_seam_lease() {
    let (core, control, _probe) = seam_core(9_513, &[]);
    let mut output = Output::attach(&core, 955);
    let (_issued, _job, outcome) = output.render(17, None);
    assert!(outcome.is_ok(), "the seam's frame exports");
    control.shutdown();
    let end = Instant::now() + WAIT;
    let mut thread = core._thread.lock().unwrap();
    while !thread.poll_join().unwrap() {
        assert!(Instant::now() < end, "shutdown must end the worker");
        std::thread::yield_now();
    }
}

/// A frame of the 16x16 target whose output damage snapshot the worker
/// records for its slot after a successful render.
fn damaged_plan(generation: u64) -> CombinedRenderPlan {
    let output = sophia_engine::HeadlessOutput {
        id: sophia_protocol::OutputId::from_raw(7),
        size: Size {
            width: 16,
            height: 16,
        },
        scale: 1,
    };
    let snapshot = sophia_engine::output_frame_damage_snapshot(
        output,
        sophia_engine::CompositorDisplayList {
            output: output.id,
            commands: Vec::new(),
        },
        &[],
        None,
    )
    .unwrap();
    let crate::PendingRenderedFrame::Mixed(mut mixed, identity) = mixed_job(generation) else {
        unreachable!("mixed_job is Mixed");
    };
    mixed.output_damage_snapshot = Some(snapshot);
    CombinedRenderPlan::new(
        Instant::now() + WAIT,
        LiveGbmEglFrameTargetRecord::new(output.size),
        crate::PendingRenderedFrame::Mixed(mixed, identity),
        Vec::new(),
        None,
        Box::new([]),
    )
}

#[test]
fn a_seam_render_settles_its_slots_damage_and_a_failed_one_clears_only_its_slot() {
    // Seven renders rotate through the three slots: 0, 1, 2, then the
    // refused fourth in slot 0, then 1, 2 and 0 again.
    let (core, _control, probe) = seam_core(9_514, &[3]);
    let mut output = Output::attach(&core, 956);
    let mut slots = Vec::new();
    for generation in 20..27 {
        let (issued, job, outcome) = output.render_plan(damaged_plan(generation));
        match outcome {
            Ok((_, token)) => {
                slots.push(Some(token.slot_id().index()));
                assert_eq!(
                    output.facade.release_combined_held(&issued, job),
                    CombinedRelease::Transferred
                );
                output.until("the held result's Release frees the slot", |slots| {
                    slots.leased == 0
                });
            }
            Err(detail) => {
                assert_eq!(
                    detail,
                    LiveRendererScanoutBufferExportDetail::ComposeRefused
                );
                slots.push(None);
            }
        }
    }
    assert_eq!(
        slots,
        [Some(0), Some(1), Some(2), None, Some(1), Some(2), Some(0)],
        "the renders took the slots in rotation"
    );
    assert_eq!(
        *probe.history.lock().unwrap(),
        [
            Some(false),
            Some(false),
            Some(false),
            Some(true),
            Some(true),
            Some(true),
            Some(false)
        ],
        "only the failed render's slot lost its history"
    );
}
