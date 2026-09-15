use super::*;
use sophia_backend_live::{
    LibdrmNativeAtomicTestPairStatus, LiveBackendRuntimeAssembly,
    LiveRenderedPrimaryPlaneScanoutCleanup, LiveRendererFrameCorrelation,
    LiveScanoutLayoutOriginalTest, LiveScanoutLayoutProbeReport,
    retry_rendered_primary_plane_scanout_cleanup, retry_scanout_layout_probe_cleanup,
};
use sophia_engine::DirectScanoutVerdict;
use sophia_renderer_live::{LiveCompositionTrace, LiveRendererScanoutBufferDescriptor};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, VecDeque},
    rc::Rc,
};

const FRAME_SIZE: Size = Size {
    width: 1280,
    height: 720,
};

struct Device {
    base: FakeNativePrimaryPlaneScanoutDevice,
    next_framebuffer: Cell<u32>,
    next_buffer: Cell<u32>,
    captured: RefCell<
        Vec<(
            drm::control::AtomicCommitFlags,
            drm::control::atomic::AtomicModeReq,
        )>,
    >,
    outcomes: RefCell<VecDeque<Option<i32>>>,
    original_add_error: Cell<Option<Option<i32>>>,
    framebuffer_modifiers: RefCell<Vec<u64>>,
    refuse_destroy: RefCell<BTreeSet<u32>>,
    refuse_close: RefCell<BTreeSet<u32>>,
    destroyed: RefCell<Vec<u32>>,
    closed: RefCell<Vec<u32>>,
}

impl Device {
    fn new(outcomes: &[Option<i32>]) -> Self {
        Self {
            base: full_primary_plane_scanout_device(),
            next_framebuffer: Cell::new(100),
            next_buffer: Cell::new(200),
            captured: RefCell::new(Vec::new()),
            outcomes: RefCell::new(outcomes.iter().copied().collect()),
            original_add_error: Cell::new(None),
            framebuffer_modifiers: RefCell::new(Vec::new()),
            refuse_destroy: RefCell::new(BTreeSet::new()),
            refuse_close: RefCell::new(BTreeSet::new()),
            destroyed: RefCell::new(Vec::new()),
            closed: RefCell::new(Vec::new()),
        }
    }

    fn framebuffer(&self) -> io::Result<drm::control::framebuffer::Handle> {
        let id = self.next_framebuffer.get();
        self.next_framebuffer.set(id + 1);
        Ok(drm::control::from_u32(id).unwrap())
    }
}
impl LibdrmNativeKmsSelectionDevice for Device {
    fn connector_handles(&self) -> io::Result<Vec<drm::control::connector::Handle>> {
        self.base.selection.connector_handles()
    }

    fn crtc_handles(&self) -> io::Result<Vec<drm::control::crtc::Handle>> {
        self.base.selection.crtc_handles()
    }

    fn connector_snapshot(
        &self,
        connector: drm::control::connector::Handle,
    ) -> io::Result<LibdrmNativeConnectorSnapshot> {
        self.base.selection.connector_snapshot(connector)
    }

    fn encoder_snapshot(
        &self,
        encoder: drm::control::encoder::Handle,
    ) -> io::Result<LibdrmNativeEncoderSnapshot> {
        self.base.selection.encoder_snapshot(encoder)
    }

    fn plane_handles(&self) -> io::Result<Vec<drm::control::plane::Handle>> {
        self.base.selection.plane_handles()
    }

    fn plane_snapshot(
        &self,
        plane: drm::control::plane::Handle,
    ) -> io::Result<LibdrmNativePlaneSnapshot> {
        self.base.selection.plane_snapshot(plane)
    }

    fn plane_type(
        &self,
        plane: drm::control::plane::Handle,
    ) -> io::Result<Option<drm::control::PlaneType>> {
        self.base.selection.plane_type(plane)
    }
}

impl LibdrmNativePropertyLookupDevice for Device {
    fn connector_property_handles(
        &self,
        connector: drm::control::connector::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.base.properties.connector_property_handles(connector)
    }

    fn crtc_property_handles(
        &self,
        crtc: drm::control::crtc::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.base.properties.crtc_property_handles(crtc)
    }

    fn plane_property_handles(
        &self,
        plane: drm::control::plane::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.base.properties.plane_property_handles(plane)
    }
}

impl LibdrmNativePrimaryPlaneResourceDevice for Device {
    fn create_mode_blob_for_selection(
        &self,
        selection: LibdrmNativePrimaryPlaneSelection,
    ) -> io::Result<u64> {
        self.base.create_mode_blob_for_selection(selection)
    }
    fn create_mode_blob(&self, mode: drm::control::Mode) -> io::Result<u64> {
        self.base.create_mode_blob(mode)
    }
    fn add_scanout_framebuffer_with_modifiers<B: drm::buffer::PlanarBuffer + ?Sized>(
        &self,
        buffer: &B,
    ) -> io::Result<drm::control::framebuffer::Handle> {
        let modifier = buffer.modifier().map(u64::from).unwrap();
        self.framebuffer_modifiers.borrow_mut().push(modifier);
        if modifier != 0
            && let Some(error) = self.original_add_error.get()
        {
            return Err(error.map_or_else(
                || {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unclassified framebuffer refusal",
                    )
                },
                io::Error::from_raw_os_error,
            ));
        }
        self.framebuffer()
    }
    fn add_scanout_framebuffer_without_modifiers<B: drm::buffer::PlanarBuffer + ?Sized>(
        &self,
        _: &B,
    ) -> io::Result<drm::control::framebuffer::Handle> {
        self.framebuffer()
    }
    fn add_legacy_scanout_framebuffer<B: drm::buffer::Buffer + ?Sized>(
        &self,
        _: &B,
        _: u32,
        _: u32,
    ) -> io::Result<drm::control::framebuffer::Handle> {
        self.framebuffer()
    }
    fn destroy_scanout_framebuffer(
        &self,
        framebuffer: drm::control::framebuffer::Handle,
    ) -> io::Result<()> {
        let id = framebuffer.into();
        if self.refuse_destroy.borrow().contains(&id) {
            return Err(io::Error::from_raw_os_error(16));
        }
        self.destroyed.borrow_mut().push(id);
        Ok(())
    }
    fn import_scanout_dma_buf(&self, _: BorrowedFd<'_>) -> io::Result<drm::buffer::Handle> {
        let id = self.next_buffer.get();
        self.next_buffer.set(id + 1);
        Ok(buffer_handle(id))
    }
    fn close_scanout_buffer(&self, handle: drm::buffer::Handle) -> io::Result<()> {
        if self.refuse_close.borrow().contains(&handle.into()) {
            return Err(io::Error::from_raw_os_error(16));
        }
        self.closed.borrow_mut().push(handle.into());
        Ok(())
    }
    fn destroy_mode_blob(&self, blob: u64) -> io::Result<()> {
        self.base.destroy_mode_blob(blob)
    }
}

impl LibdrmNativeAtomicCommitDevice for Device {
    fn submit_atomic_commit(
        &self,
        flags: drm::control::AtomicCommitFlags,
        request: drm::control::atomic::AtomicModeReq,
    ) -> io::Result<()> {
        self.captured.borrow_mut().push((flags, request));
        self.outcomes
            .borrow_mut()
            .pop_front()
            .expect("unexpected extra atomic commit")
            .map_or(Ok(()), |errno| Err(io::Error::from_raw_os_error(errno)))
    }
}

#[derive(Debug)]
struct Owner {
    id: u32,
    direct: bool,
    dropped: Rc<RefCell<Vec<u32>>>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.dropped.borrow_mut().push(self.id);
    }
}

impl LiveRenderedScanoutBufferPrimeSource for Owner {
    fn is_direct_client_buffer(&self) -> bool {
        self.direct
    }
    fn shares_kms_drm_file(&self) -> bool {
        false
    }
    fn export_scanout_dma_buf_fds(&self) -> io::Result<Option<LiveRenderedScanoutDmaBufFds>> {
        Ok(Some(LiveRenderedScanoutDmaBufFds::new_for_test(
            [
                Some(std::fs::File::open("/dev/null")?.into()),
                None,
                None,
                None,
            ],
            1,
        )))
    }
}

struct Exporter {
    output: Option<LiveRenderedScanoutBufferExport<Owner>>,
    source: Option<LiveRenderedScanoutBufferExport<Owner>>,
    cleanup: Option<LiveRenderedPrimaryPlaneScanoutCleanup<Owner>>,
    reports: Vec<LiveScanoutLayoutProbeReport>,
    dropped: Rc<RefCell<Vec<u32>>>,
    offers: usize,
}

impl Exporter {
    fn new() -> Self {
        let dropped = Rc::new(RefCell::new(Vec::new()));
        Self {
            output: Some(export(1, false, 0, Some(correlation(false)), &dropped)),
            source: Some(export(2, true, 7, Some(correlation(true)), &dropped)),
            cleanup: None,
            reports: Vec::new(),
            dropped,
            offers: 0,
        }
    }
}

impl LiveRenderedScanoutBufferExporter for Exporter {
    type Owner = Owner;
    fn export_rendered_scanout_buffer(
        &mut self,
        _: LiveGbmEglFrameTargetRecord,
    ) -> LiveRenderedScanoutBufferExport<Owner> {
        self.output
            .take()
            .expect("ordinary frame was requested twice")
    }
    fn layout_probe_cleanup(
        &mut self,
    ) -> Option<&mut Option<LiveRenderedPrimaryPlaneScanoutCleanup<Owner>>> {
        Some(&mut self.cleanup)
    }
    fn take_layout_probe_source(
        &mut self,
        _: Option<LiveRendererFrameCorrelation>,
        _: LiveRendererScanoutBufferDescriptor,
    ) -> Option<sophia_backend_live::LiveScanoutLayoutProbeSource<Owner>> {
        self.offers += 1;
        self.source
            .take()
            .map(|export| sophia_backend_live::LiveScanoutLayoutProbeSource {
                export,
                image: sophia_renderer_live::LiveRendererImageId::from_raw(8812),
            })
    }
    fn record_layout_probe(&mut self, report: LiveScanoutLayoutProbeReport) {
        self.reports.push(report);
    }
}

fn correlation(original: bool) -> LiveRendererFrameCorrelation {
    LiveRendererFrameCorrelation {
        native: None,
        request: None,
        trace: Some(LiveCompositionTrace {
            output: OutputId::from_raw(1),
            head: RenderHeadId::from_raw(2),
            scene_generation: 3,
        }),
        direct_scanout: Some(if original {
            DirectScanoutVerdict::Eligible
        } else {
            DirectScanoutVerdict::CompositionRequired("refused")
        }),
    }
}

fn export(
    id: u32,
    direct: bool,
    modifier: u64,
    correlation: Option<LiveRendererFrameCorrelation>,
    dropped: &Rc<RefCell<Vec<u32>>>,
) -> LiveRenderedScanoutBufferExport<Owner> {
    let mut descriptor = scanout_descriptor(FRAME_SIZE);
    descriptor.modifier = Some(modifier);
    LiveRenderedScanoutBufferExport::new(
        LiveRendererScanoutBufferExportStatus::Exported,
        LiveRendererScanoutBufferExportDetail::Exported,
        Some(descriptor),
        Some(Owner {
            id,
            direct,
            dropped: Rc::clone(dropped),
        }),
    )
    .with_correlation(correlation)
}

fn assembly(name: &str) -> LiveBackendRuntimeAssembly {
    let root = ready_drm_sysfs_fixture(name);
    discover_live_backend(&LiveBackendConfig::new(&root))
        .into_live_runtime_assembly(QueuedInputPoller::default())
        .unwrap()
}

fn retire(
    device: &Device,
    submission: sophia_backend_live::LiveRenderedPrimaryPlaneScanoutSubmission<Owner>,
) {
    let retired = retire_rendered_primary_plane_scanout_after_page_flip(
        device,
        submission,
        &LivePageFlipCallbackReport {
            decision: LivePageFlipCallbackDecision::Accepted,
            event: LivePageFlipEvent {
                status: LivePageFlipEventStatus::Presented,
                frame_serial: Some(55),
            },
        },
    );
    assert_eq!(
        retired.status,
        LibdrmNativePrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip
    );
    assert!(retired.cleanup.is_none());
}

fn assert_captured_matches(
    device: &Device,
    index: usize,
    evidence: sophia_backend_live::LibdrmNativeAtomicRequestEvidence,
) {
    let mut expected = drm::control::atomic::AtomicModeReq::new();
    for row in evidence.properties() {
        expected.add_raw_property(
            drm::control::from_u32(row.object).unwrap(),
            property_handle(row.property),
            row.value,
        );
    }
    // The raw request exposes no iterator/equality; Debug comparison is confined to this test.
    assert_eq!(
        format!("{:?}", device.captured.borrow()[index].1),
        format!("{expected:?}")
    );
}

#[test]
fn layout_probe_tests_the_pair_then_commits_the_existing_alternative_with_unchanged_cursor() {
    let mut assembly = assembly("layout-probe-pair");
    let output = assembly
        .rendered_outputs()
        .outputs()
        .next()
        .unwrap()
        .output();
    assert!(assembly.set_cursor_ride_request(
        output,
        Some(sophia_backend_live::LibdrmNativeAtomicCursor {
            plane: drm::control::from_u32(61).unwrap(),
            properties: cursor_plane_property_handles(),
            placement: Some(sophia_backend_live::LibdrmNativeCursorPlacement {
                framebuffer: drm::control::from_u32(9).unwrap(),
                x: 40,
                y: 30,
                width: 64,
                height: 64,
            }),
        })
    ));
    let device = Device::new(&[Some(22), None, None]);
    let mut exporter = Exporter::new();
    let submitted = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(
        submitted.status,
        LiveRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    assert_eq!(device.captured.borrow().len(), 3);
    assert!(device.outcomes.borrow().is_empty());
    assert_eq!(exporter.reports.len(), 1);
    let report = exporter.reports[0];
    assert_eq!(
        report.tests.status,
        LibdrmNativeAtomicTestPairStatus::Tested
    );
    let LiveScanoutLayoutOriginalTest::Atomic(original) = report.tests.original.unwrap() else {
        panic!("a prepared original must retain its actual atomic test");
    };
    let alternative = report.tests.alternative.unwrap();
    assert_eq!(original.raw_os_error, Some(22));
    assert_eq!(alternative.raw_os_error, None);
    let original = original.request.unwrap();
    let alternative = alternative.request.unwrap();
    assert!(original.equivalent_except_primary_framebuffer(&alternative));
    assert!(
        original.properties().iter().any(|row| row.object == 61),
        "the probe silently dropped the cursor"
    );
    assert_captured_matches(&device, 0, original);
    assert_captured_matches(&device, 1, alternative);
    assert_captured_matches(&device, 2, alternative);
    for index in [0, 1] {
        assert!(
            device.captured.borrow()[index]
                .0
                .contains(drm::control::AtomicCommitFlags::TEST_ONLY)
        );
    }
    assert!(
        !device.captured.borrow()[2]
            .0
            .contains(drm::control::AtomicCommitFlags::TEST_ONLY)
    );
    assert!(
        device.captured.borrow()[2]
            .0
            .contains(drm::control::AtomicCommitFlags::PAGE_FLIP_EVENT)
    );
    assert_eq!(*device.destroyed.borrow(), [101]);
    assert_eq!(*device.closed.borrow(), [201]);
    assert_eq!(*exporter.dropped.borrow(), [2]);
    assert!(exporter.cleanup.is_none());
    retire(&device, submitted.submission.unwrap());
    assert_eq!(*device.destroyed.borrow(), [101, 100]);
    assert_eq!(*device.closed.borrow(), [201, 200]);
    assert_eq!(*exporter.dropped.borrow(), [2, 1]);
}

#[test]
fn layout_probe_rejects_unknown_or_incomparable_evidence_before_extra_preparation() {
    for case in 0..8 {
        let mut assembly = assembly(&format!("layout-probe-incomparable-{case}"));
        let device = Device::new(&[None]);
        let mut exporter = Exporter::new();
        let source = exporter.source.as_mut().unwrap();
        match case {
            0 => source.descriptor.as_mut().unwrap().format = LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
            1 => {
                source
                    .correlation
                    .as_mut()
                    .unwrap()
                    .trace
                    .as_mut()
                    .unwrap()
                    .scene_generation += 1
            }
            2 => source.correlation = None,
            3 => source.correlation.as_mut().unwrap().trace = None,
            4 => exporter.output.as_mut().unwrap().correlation = None,
            5 => source.descriptor.as_mut().unwrap().modifier = Some(0),
            6 => source.descriptor.as_mut().unwrap().modifier = None,
            7 => {
                source.correlation.as_mut().unwrap().direct_scanout =
                    Some(DirectScanoutVerdict::LayerNotActive)
            }
            _ => unreachable!(),
        }
        let submitted = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
        assert_eq!(
            submitted.status,
            LiveRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip,
            "case {case}"
        );
        assert!(
            exporter.reports.is_empty(),
            "case {case} fabricated a layout comparison"
        );
        assert_eq!(device.captured.borrow().len(), 1);
        assert_eq!(
            device.next_framebuffer.get(),
            101,
            "case {case} prepared an incomparable source"
        );
        assert_eq!(device.next_buffer.get(), 201);
        assert!(exporter.cleanup.is_none());
        retire(&device, submitted.submission.unwrap());
    }
}

#[test]
fn optional_original_cleanup_and_failed_real_submission_keep_independent_retry_owners() {
    let mut assembly = assembly("layout-probe-independent-cleanup");
    let device = Device::new(&[Some(22), None, Some(5), None]);
    device.refuse_destroy.borrow_mut().extend([100, 101]);
    let mut exporter = Exporter::new();
    let failed = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(
        failed.status,
        LiveRenderedPrimaryPlaneScanoutSubmitStatus::PrimaryPlaneSubmitFailed
    );
    let ordinary_cleanup = failed.cleanup.expect("failed real submit lost its owner");
    assert!(
        exporter.cleanup.is_some(),
        "ordinary failure replaced optional cleanup"
    );
    assert!(device.destroyed.borrow().is_empty());
    assert!(device.closed.borrow().is_empty());
    assert!(exporter.dropped.borrow().is_empty());
    assert_eq!(exporter.reports.len(), 1);

    exporter.output = Some(export(
        3,
        false,
        0,
        Some(correlation(false)),
        &exporter.dropped,
    ));
    let succeeding = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(
        succeeding.status,
        LiveRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    assert_eq!(
        device.captured.borrow().len(),
        4,
        "pending optional cleanup blocked or added work to normal submit"
    );
    assert_eq!(exporter.reports.len(), 1);
    assert_eq!(
        exporter.offers, 1,
        "pending cleanup admitted another optional source"
    );
    assert!(exporter.cleanup.is_some());

    device.refuse_destroy.borrow_mut().clear();
    assert!(!retry_scanout_layout_probe_cleanup(&device, &mut exporter));
    assert_eq!(*device.destroyed.borrow(), [101]);
    assert_eq!(*device.closed.borrow(), [201]);
    assert_eq!(*exporter.dropped.borrow(), [2]);
    assert!(!retry_scanout_layout_probe_cleanup(&device, &mut exporter));
    let retried = retry_rendered_primary_plane_scanout_cleanup(&device, ordinary_cleanup);
    assert!(retried.cleanup.is_none());
    assert_eq!(*device.destroyed.borrow(), [101, 100]);
    assert_eq!(*device.closed.borrow(), [201, 200]);
    assert_eq!(*exporter.dropped.borrow(), [2, 1]);
    retire(&device, succeeding.submission.unwrap());
    assert_eq!(*device.destroyed.borrow(), [101, 100, 102]);
    assert_eq!(*device.closed.borrow(), [201, 200, 202]);
    assert_eq!(*exporter.dropped.borrow(), [2, 1, 3]);
    assert!(device.outcomes.borrow().is_empty());
}

#[test]
fn framebuffer_refusal_tests_only_the_existing_alternative_and_retires_its_witness() {
    let mut assembly = assembly("layout-probe-framebuffer");
    let output = assembly
        .rendered_outputs()
        .outputs()
        .next()
        .unwrap()
        .output();
    assert!(assembly.set_cursor_ride_request(
        output,
        Some(sophia_backend_live::LibdrmNativeAtomicCursor {
            plane: drm::control::from_u32(61).unwrap(),
            properties: cursor_plane_property_handles(),
            placement: Some(sophia_backend_live::LibdrmNativeCursorPlacement {
                framebuffer: drm::control::from_u32(9).unwrap(),
                x: 40,
                y: 30,
                width: 64,
                height: 64,
            }),
        })
    ));
    let device = Device::new(&[None, None]);
    device.original_add_error.set(Some(Some(22)));
    let mut exporter = Exporter::new();
    let submitted = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(
        submitted.status,
        LiveRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    assert_eq!(*device.framebuffer_modifiers.borrow(), [0, 7]);
    assert_eq!(device.next_buffer.get(), 202, "both PRIME imports ran");
    assert_eq!(
        device.next_framebuffer.get(),
        101,
        "only the alternative owns an FB"
    );
    assert_eq!(exporter.reports.len(), 1);
    let report = exporter.reports[0];
    assert_eq!(
        report.tests.status,
        LibdrmNativeAtomicTestPairStatus::Tested
    );
    let LiveScanoutLayoutOriginalTest::Framebuffer {
        intended_request,
        error_kind,
        raw_os_error,
    } = report.tests.original.unwrap()
    else {
        panic!("AddFB2 refusal cannot impersonate an atomic test");
    };
    assert_eq!(error_kind, io::ErrorKind::InvalidInput);
    assert_eq!(raw_os_error, Some(22));
    let alternative = report.tests.alternative.unwrap();
    assert_eq!(alternative.raw_os_error, None);
    let intended = intended_request.unwrap();
    assert_eq!(Some(intended), alternative.request);
    assert!(intended.properties().iter().any(|row| row.object == 61));
    assert_eq!(device.captured.borrow().len(), 2);
    assert_captured_matches(&device, 0, intended);
    assert_captured_matches(&device, 1, intended);
    assert!(
        device.captured.borrow()[0]
            .0
            .contains(drm::control::AtomicCommitFlags::TEST_ONLY)
    );
    assert!(
        !device.captured.borrow()[1]
            .0
            .contains(drm::control::AtomicCommitFlags::TEST_ONLY)
    );
    assert!(
        device.captured.borrow()[1]
            .0
            .contains(drm::control::AtomicCommitFlags::PAGE_FLIP_EVENT)
    );
    let witness = submitted
        .layout_witness
        .expect("fresh framebuffer comparison");
    assert_eq!(witness.source_image, report.source_image);
    assert_eq!(witness.alternative, correlation(false));
    assert_eq!(
        (witness.original_modifier, witness.alternative_modifier),
        (7, 0)
    );
    assert!(device.destroyed.borrow().is_empty());
    assert_eq!(*device.closed.borrow(), [201]);
    assert_eq!(*exporter.dropped.borrow(), [2]);
    let callback = LivePageFlipCallbackReport {
        decision: LivePageFlipCallbackDecision::RejectedStaleFrameSerial,
        event: LivePageFlipEvent {
            status: LivePageFlipEventStatus::Presented,
            frame_serial: Some(55),
        },
    };
    let waiting = retire_rendered_primary_plane_scanout_after_page_flip(
        &device,
        submitted.submission.unwrap(),
        &callback,
    );
    assert!(waiting.layout_witness.is_none());
    assert!(waiting.cleanup.is_none());
    let waiting = waiting
        .submission
        .expect("stale callback retained the alternative");
    assert_eq!(waiting.layout_witness(), Some(witness));
    let retired = retire_rendered_primary_plane_scanout_after_page_flip(
        &device,
        waiting,
        &LivePageFlipCallbackReport {
            decision: LivePageFlipCallbackDecision::Accepted,
            ..callback
        },
    );
    assert_eq!(
        retired.status,
        LibdrmNativePrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip
    );
    assert_eq!(retired.layout_witness, Some(witness));
    assert!(retired.submission.is_none() && retired.cleanup.is_none());
    assert_eq!(*device.destroyed.borrow(), [100]);
    assert_eq!(*device.closed.borrow(), [201, 200]);
    assert_eq!(*exporter.dropped.borrow(), [2, 1]);
    assert!(device.outcomes.borrow().is_empty());
}

#[test]
fn framebuffer_error_classification_never_turns_other_failures_into_layout_evidence() {
    for (index, (errno, alternative_failure)) in [
        (Some(22), None),
        (Some(16), None),
        (Some(12), None),
        (Some(13), None),
        (None, None),
        (Some(22), Some(22)),
    ]
    .into_iter()
    .enumerate()
    {
        let mut assembly = assembly(&format!("layout-probe-framebuffer-error-{index}"));
        let eligible = errno == Some(22);
        let outcomes = if eligible {
            vec![alternative_failure, None]
        } else {
            vec![None]
        };
        let device = Device::new(&outcomes);
        device.original_add_error.set(Some(errno));
        let mut exporter = Exporter::new();
        let submitted = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
        assert_eq!(
            submitted.status,
            LiveRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip,
            "case {index}"
        );
        assert_eq!(exporter.reports.len(), 1);
        let report = exporter.reports[0];
        let LiveScanoutLayoutOriginalTest::Framebuffer { raw_os_error, .. } =
            report.tests.original.unwrap()
        else {
            panic!("case {index}: framebuffer failure relabelled as atomic");
        };
        assert_eq!(raw_os_error, errno);
        assert_eq!(report.tests.alternative.is_some(), eligible);
        assert_eq!(
            report.tests.status,
            if eligible {
                LibdrmNativeAtomicTestPairStatus::Tested
            } else {
                LibdrmNativeAtomicTestPairStatus::FramebufferRejectionIneligible
            }
        );
        assert_eq!(
            submitted.layout_witness.is_some(),
            eligible && alternative_failure.is_none(),
            "case {index}"
        );
        assert_eq!(device.captured.borrow().len(), outcomes.len());
        assert_eq!(
            device
                .captured
                .borrow()
                .iter()
                .filter(|(flags, _)| flags.contains(drm::control::AtomicCommitFlags::TEST_ONLY))
                .count(),
            usize::from(eligible)
        );
        assert_eq!(*device.framebuffer_modifiers.borrow(), [0, 7]);
        assert_eq!(*device.closed.borrow(), [201]);
        assert_eq!(*exporter.dropped.borrow(), [2]);
        retire(&device, submitted.submission.unwrap());
        assert_eq!(*device.destroyed.borrow(), [100]);
        assert_eq!(*device.closed.borrow(), [201, 200]);
        assert_eq!(*exporter.dropped.borrow(), [2, 1]);
        assert!(device.outcomes.borrow().is_empty());
    }
}

#[test]
fn failed_original_gem_cleanup_survives_independently_of_failed_alternative_commit() {
    let mut assembly = assembly("layout-probe-framebuffer-cleanup");
    let device = Device::new(&[None, Some(5), None]);
    device.original_add_error.set(Some(Some(22)));
    device.refuse_close.borrow_mut().insert(201);
    device.refuse_destroy.borrow_mut().insert(100);
    let mut exporter = Exporter::new();
    let failed = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(
        failed.status,
        LiveRenderedPrimaryPlaneScanoutSubmitStatus::PrimaryPlaneSubmitFailed
    );
    assert!(failed.layout_witness.is_none());
    let ordinary_cleanup = failed.cleanup.expect("failed alternative lost its owner");
    assert!(exporter.cleanup.is_some(), "original GEM cleanup was lost");
    assert!(device.closed.borrow().is_empty());
    assert!(device.destroyed.borrow().is_empty());
    assert!(exporter.dropped.borrow().is_empty());
    exporter.output = Some(export(
        3,
        false,
        0,
        Some(correlation(false)),
        &exporter.dropped,
    ));
    let succeeding = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(
        succeeding.status,
        LiveRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    assert_eq!(device.captured.borrow().len(), 3);
    assert_eq!(
        exporter.offers, 1,
        "outstanding debt admitted a new optional source"
    );
    assert_eq!(exporter.reports.len(), 1);
    assert!(exporter.cleanup.is_some());
    device.refuse_close.borrow_mut().clear();
    device.refuse_destroy.borrow_mut().clear();
    assert!(!retry_scanout_layout_probe_cleanup(&device, &mut exporter));
    assert_eq!(*device.closed.borrow(), [201]);
    assert!(
        device.destroyed.borrow().is_empty(),
        "a rejected FB never existed"
    );
    assert_eq!(*exporter.dropped.borrow(), [2]);
    assert!(!retry_scanout_layout_probe_cleanup(&device, &mut exporter));
    assert_eq!(*device.closed.borrow(), [201], "GEM released twice");
    assert!(
        retry_rendered_primary_plane_scanout_cleanup(&device, ordinary_cleanup)
            .cleanup
            .is_none()
    );
    assert_eq!(*device.destroyed.borrow(), [100]);
    assert_eq!(*device.closed.borrow(), [201, 200]);
    assert_eq!(*exporter.dropped.borrow(), [2, 1]);
    retire(&device, succeeding.submission.unwrap());
    assert_eq!(*device.destroyed.borrow(), [100, 101]);
    assert_eq!(*device.closed.borrow(), [201, 200, 202]);
    assert_eq!(*exporter.dropped.borrow(), [2, 1, 3]);
    assert!(device.outcomes.borrow().is_empty());
}

#[path = "rendered_layout_witness.rs"]
mod witness;
