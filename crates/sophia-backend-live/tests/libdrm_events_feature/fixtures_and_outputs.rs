fn clone_io_result<T: Clone>(result: &io::Result<T>) -> io::Result<T> {
    result
        .as_ref()
        .cloned()
        .map_err(|error| io::Error::new(error.kind(), "synthetic property lookup failure"))
}

fn property_handle(raw: u32) -> drm::control::property::Handle {
    drm::control::from_u32(raw).expect("test property handle should be nonzero")
}

fn connector_handle() -> drm::control::connector::Handle {
    drm::control::from_u32(11).expect("test connector handle should be nonzero")
}

fn crtc_handle() -> drm::control::crtc::Handle {
    drm::control::from_u32(12).expect("test crtc handle should be nonzero")
}

fn encoder_handle() -> drm::control::encoder::Handle {
    drm::control::from_u32(16).expect("test encoder handle should be nonzero")
}

fn plane_handle() -> drm::control::plane::Handle {
    drm::control::from_u32(13).expect("test plane handle should be nonzero")
}

fn framebuffer_handle() -> drm::control::framebuffer::Handle {
    drm::control::from_u32(14).expect("test framebuffer handle should be nonzero")
}

fn buffer_handle(raw: u32) -> drm::buffer::Handle {
    drm::control::from_u32(raw).expect("test buffer handle should be nonzero")
}

fn primary_plane_properties() -> LibdrmNativePrimaryPlanePropertyHandles {
    LibdrmNativePrimaryPlanePropertyHandles::new(
        property_handle(101),
        property_handle(102),
        property_handle(103),
        property_handle(104),
        property_handle(105),
        property_handle(106),
        property_handle(107),
        property_handle(108),
        property_handle(109),
        property_handle(110),
        property_handle(111),
        property_handle(112),
        property_handle(113),
    )
}

fn primary_plane_objects(size: Size) -> LibdrmNativePrimaryPlaneObjects {
    LibdrmNativePrimaryPlaneObjects::new(
        connector_handle(),
        crtc_handle(),
        plane_handle(),
        framebuffer_handle(),
        15,
        size,
    )
}

fn full_property_lookup_device() -> FakeNativePropertyLookupDevice {
    FakeNativePropertyLookupDevice {
        connector: Ok(LibdrmNativePropertyHandleSet::new([(
            "CRTC_ID",
            property_handle(101),
        )])),
        crtc: Ok(LibdrmNativePropertyHandleSet::new([
            ("MODE_ID", property_handle(102)),
            ("ACTIVE", property_handle(103)),
        ])),
        plane: Ok(LibdrmNativePropertyHandleSet::new([
            ("FB_ID", property_handle(104)),
            ("CRTC_ID", property_handle(105)),
            ("SRC_X", property_handle(106)),
            ("SRC_Y", property_handle(107)),
            ("SRC_W", property_handle(108)),
            ("SRC_H", property_handle(109)),
            ("CRTC_X", property_handle(110)),
            ("CRTC_Y", property_handle(111)),
            ("CRTC_W", property_handle(112)),
            ("CRTC_H", property_handle(113)),
            ("IN_FORMATS", property_handle(114)),
        ])),
        connector_value: Ok(None),
    }
}

fn kms_selection_device_with_mode_size(size: Size) -> FakeNativeKmsSelectionDevice {
    FakeNativeKmsSelectionDevice {
        connectors: Ok(vec![connector_handle()]),
        crtcs: Ok(vec![crtc_handle()]),
        planes: Ok(vec![plane_handle()]),
        connector_snapshot: Ok(LibdrmNativeConnectorSnapshot::new(
            true,
            Some(encoder_handle()),
            [encoder_handle()],
            Some(size),
        )),
        encoder_snapshot: Ok(LibdrmNativeEncoderSnapshot::new(
            Some(crtc_handle()),
            [crtc_handle()],
        )),
        plane_snapshot: Ok(LibdrmNativePlaneSnapshot::new([crtc_handle()])),
        plane_type: Ok(Some(drm::control::PlaneType::Primary)),
    }
}

fn full_kms_selection_device() -> FakeNativeKmsSelectionDevice {
    kms_selection_device_with_mode_size(Size {
        width: 1280,
        height: 720,
    })
}

fn full_primary_plane_resource_device() -> FakeNativePrimaryPlaneResourceDevice {
    FakeNativePrimaryPlaneResourceDevice {
        destroyed_framebuffers: std::cell::Cell::new(0),
        imported_buffers: std::cell::Cell::new(0),
        closed_buffers: std::cell::Cell::new(0),
        mode_blob: Ok(15),
        framebuffer: Ok(framebuffer_handle()),
        destroy_framebuffer: Ok(()),
        destroy_mode_blob: Ok(()),
    }
}

fn full_prime_primary_plane_resource_device() -> FakePrimePrimaryPlaneResourceDevice {
    let imported = buffer_handle(33);
    FakePrimePrimaryPlaneResourceDevice {
        mode_blob: Ok(15),
        framebuffer: Ok(framebuffer_handle()),
        imported_buffer: Ok(imported),
        close_buffer: Ok(()),
        destroy_framebuffer: Ok(()),
        destroy_mode_blob: Ok(()),
        expected_framebuffer_buffer: Some(imported),
    }
}

fn test_dma_buf_plane_fds() -> [Option<OwnedFd>; 4] {
    let fd: OwnedFd = std::fs::File::open("/dev/null")
        .expect("test host should expose /dev/null")
        .into();
    [Some(fd), None, None, None]
}

fn full_primary_plane_scanout_device() -> FakeNativePrimaryPlaneScanoutDevice {
    FakeNativePrimaryPlaneScanoutDevice {
        commits: std::cell::Cell::new(0),
        test_only_commits: std::cell::Cell::new(0),
        accept_commits: None,
        reject_commits_before: 0,
        selection: full_kms_selection_device(),
        properties: full_property_lookup_device(),
        resources: full_primary_plane_resource_device(),
        submit: Ok(()),
    }
}

fn scanout_descriptor(size: Size) -> sophia_renderer_live::LiveRendererScanoutBufferDescriptor {
    let mut exporter =
        FakeRendererScanoutBufferExporter::new(LiveRendererScanoutBufferExportStatus::Exported)
            .with_descriptor(
                size.width as u32 * 4,
                LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                17,
            );

    exporter
        .export_scanout_buffer(LiveGbmEglFrameTargetRecord::new(size))
        .descriptor
        .expect("ready fake renderer export should include a scanout descriptor")
}

fn scanout_buffer(size: Size) -> LibdrmRendererScanoutBuffer {
    LibdrmRendererScanoutBuffer::from_descriptor(scanout_descriptor(size))
        .expect("ready renderer descriptor should become a backend-private DRM buffer")
}

struct FakeDrmBuffer {
    size: (u32, u32),
    pitch: u32,
    format: drm::buffer::DrmFourcc,
    handle: drm::buffer::Handle,
    plane_handles: [Option<drm::buffer::Handle>; 4],
    plane_pitches: [u32; 4],
    plane_offsets: [u32; 4],
    modifier: Option<drm::buffer::DrmModifier>,
}

impl FakeDrmBuffer {
    fn xrgb8888(size: Size) -> Self {
        Self {
            size: (size.width as u32, size.height as u32),
            pitch: size.width as u32 * 4,
            format: drm::buffer::DrmFourcc::Xrgb8888,
            handle: drm::control::from_u32(17).expect("test buffer handle should be nonzero"),
            plane_handles: [
                Some(drm::control::from_u32(17).expect("test buffer handle should be nonzero")),
                None,
                None,
                None,
            ],
            plane_pitches: [size.width as u32 * 4, 0, 0, 0],
            plane_offsets: [0, 0, 0, 0],
            modifier: None,
        }
    }

    fn with_pitch(mut self, pitch: u32) -> Self {
        self.pitch = pitch;
        self.plane_pitches[0] = pitch;
        self
    }

    fn with_format(mut self, format: drm::buffer::DrmFourcc) -> Self {
        self.format = format;
        self
    }

    fn with_two_planes(mut self) -> Self {
        self.plane_handles[1] =
            Some(drm::control::from_u32(18).expect("test buffer handle should be nonzero"));
        self.plane_pitches[1] = self.pitch;
        self
    }

    fn with_modifier(mut self, modifier: drm::buffer::DrmModifier) -> Self {
        self.modifier = Some(modifier);
        self
    }
}

impl drm::buffer::Buffer for FakeDrmBuffer {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn format(&self) -> drm::buffer::DrmFourcc {
        self.format
    }

    fn pitch(&self) -> u32 {
        self.pitch
    }

    fn handle(&self) -> drm::buffer::Handle {
        self.handle
    }
}

impl drm::buffer::PlanarBuffer for FakeDrmBuffer {
    fn size(&self) -> (u32, u32) {
        drm::buffer::Buffer::size(self)
    }

    fn format(&self) -> drm::buffer::DrmFourcc {
        drm::buffer::Buffer::format(self)
    }

    fn modifier(&self) -> Option<drm::buffer::DrmModifier> {
        self.modifier
    }

    fn pitches(&self) -> [u32; 4] {
        self.plane_pitches
    }

    fn handles(&self) -> [Option<drm::buffer::Handle>; 4] {
        self.plane_handles
    }

    fn offsets(&self) -> [u32; 4] {
        self.plane_offsets
    }
}

#[derive(Debug, Eq, PartialEq)]
struct FakeRenderedScanoutOwner {
    raw: u32,
}

impl LiveRenderedScanoutBufferPrimeSource for FakeRenderedScanoutOwner {
    fn shares_kms_drm_file(&self) -> bool {
        true
    }

    fn export_scanout_dma_buf_fds(&self) -> io::Result<Option<LiveRenderedScanoutDmaBufFds>> {
        Ok(None)
    }
}

struct FakeRenderedScanoutExporter {
    status: LiveRendererScanoutBufferExportStatus,
    descriptor: Option<sophia_renderer_live::LiveRendererScanoutBufferDescriptor>,
    owner: Option<FakeRenderedScanoutOwner>,
    export_attempts: usize,
}

#[cfg(feature = "gbm-probe")]
struct MissingRenderDevice;

#[cfg(feature = "gbm-probe")]
impl RenderDeviceDiscoveryBackend for MissingRenderDevice {
    type Device = std::fs::File;

    fn open_render_device(&self) -> io::Result<Self::Device> {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "test render device unavailable",
        ))
    }
}

#[cfg(feature = "gbm-probe")]
#[test]
fn pending_rendered_frame_is_a_latest_frame_wins_slot() {
    let mut exporter = NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingRenderDevice);
    exporter.set_pending_cpu_frame(LiveCpuComposedFrame {
        size: Size {
            width: 2,
            height: 2,
        },
        stride: 8,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        bytes: vec![0; 16].into(),
    });
    assert!(exporter.pending_cpu_frame());

    exporter.set_pending_mixed_frame(sophia_renderer_live::LiveOwnedMixedCompositionFrame {
        layers: Vec::new(),
        output_damage_snapshot: None,
        trace: None,
        direct_scanout: Default::default(),
    });
    assert!(!exporter.pending_cpu_frame());
    assert!(exporter.pending_mixed_frame());
    assert!(exporter.pending_frame());
}

#[cfg(feature = "gbm-probe")]
#[test]
fn hardware_cursor_mode_never_enters_cpu_composition() {
    let position = sophia_protocol::Point { x: 12.0, y: 34.0 };

    assert_eq!(
        LiveProductionCursorPresentation::HardwarePlane.composition_position(),
        None
    );
    assert_eq!(
        LiveProductionCursorPresentation::Software(Some(position)).composition_position(),
        Some(position)
    );
}

/// Stability is a claim about one transaction's own page flip.
///
/// The predicate once also took what was submitted and whether any head was
/// busy, and refused stability whenever either said work had arrived since.
/// That asked a flowing pipeline for a quiescent instant it does not have: a
/// physical mixed-topology run judged all eleven of its retirements superseded
/// and never reported readiness. Successor state is no longer an argument, so
/// there is nothing left here to vary -- the signature is the invariant. What
/// stays falsifiable is everything about the flip itself.
#[cfg(feature = "gbm-probe")]
#[test]
fn stable_present_requires_this_transaction_displayed_with_real_pixels() {
    let transaction = TransactionId::from_raw(41);
    let displayed = Some(LiveProductionScanoutContent::MixedPresent {
        frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(1),
        transaction,
        nonzero_rgb_pixels: 1,
    });

    assert!(live_production_scanout_is_stable_present(
        displayed,
        transaction
    ));
    // A flip that put nothing visible on the screen is not this client's
    // pixels arriving, however exactly it names the transaction.
    assert!(!live_production_scanout_is_stable_present(
        Some(LiveProductionScanoutContent::MixedPresent {
            frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(3),
            transaction,
            nonzero_rgb_pixels: 0,
        }),
        transaction,
    ));
    // Some other transaction's pixels are on the screen, not this one's.
    assert!(!live_production_scanout_is_stable_present(
        displayed,
        TransactionId::from_raw(42)
    ));
    // Retained content carries no transaction, so it can never be evidence
    // that a particular one was shown.
    assert!(!live_production_scanout_is_stable_present(
        Some(LiveProductionScanoutContent::RetainedMixed {
 logical_content_checksum: None,
 requires_retirement: false,
            frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(4),
            nonzero_rgb_pixels: 1,
        }),
        transaction,
    ));
    assert!(!live_production_scanout_is_stable_present(None, transaction));
}

#[cfg(feature = "gbm-probe")]
#[test]
fn production_output_runtime_resolves_primary_by_output_identity() {
    let outputs = [
        sophia_engine::HeadlessOutput {
            id: OutputId::from_raw(9),
            size: Size {
                width: 1920,
                height: 1080,
            },
            scale: 1,
        },
        sophia_engine::HeadlessOutput {
            id: OutputId::from_raw(3),
            size: Size {
                width: 2560,
                height: 1440,
            },
            scale: 1,
        },
    ];
    let runtimes = LiveProductionOutputRuntimeSet::new(&outputs, &[], None).unwrap();

    assert_eq!(runtimes.primary_output(), Some(OutputId::from_raw(3)));
    assert_eq!(runtimes.output_index(OutputId::from_raw(3)), Some(0));
    assert_eq!(runtimes.output_index(OutputId::from_raw(9)), Some(1));
}

#[cfg(feature = "gbm-probe")]
#[test]
fn production_output_runtime_replaces_complete_root_space_viewports_transactionally() {
    let outputs = [
        sophia_engine::HeadlessOutput {
            id: OutputId::from_raw(3),
            size: Size {
                width: 2560,
                height: 1440,
            },
            scale: 1,
        },
        sophia_engine::HeadlessOutput {
            id: OutputId::from_raw(9),
            size: Size {
                width: 1920,
                height: 1080,
            },
            scale: 1,
        },
    ];
    let mut runtimes = LiveProductionOutputRuntimeSet::new(&outputs, &[], None).unwrap();
    let replacement = [
        (
            OutputId::from_raw(3),
            Rect {
                x: -2560,
                y: 180,
                width: 2560,
                height: 1440,
            },
        ),
        (
            OutputId::from_raw(9),
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ),
    ];

    runtimes.replace_logical_viewports(&replacement).unwrap();

    assert_eq!(
        runtimes.logical_viewport(OutputId::from_raw(3)),
        Some(replacement[0].1)
    );
    assert_eq!(
        runtimes.logical_viewport(OutputId::from_raw(9)),
        Some(replacement[1].1)
    );
    assert!(
        runtimes
            .replace_logical_viewports(&[replacement[0]])
            .is_err()
    );
    assert_eq!(
        runtimes.logical_viewport(OutputId::from_raw(9)),
        Some(replacement[1].1),
        "a rejected partial replacement must preserve the published layout"
    );
}

#[cfg(feature = "gbm-probe")]
#[test]
fn hardware_cursor_uses_exact_root_space_viewport_offsets() {
    let left = OutputId::from_raw(3);
    let raised = OutputId::from_raw(9);
    let viewports = [
        (
            left,
            Rect {
                x: -2560,
                y: 180,
                width: 2560,
                height: 1440,
            },
        ),
        (
            raised,
            Rect {
                x: 0,
                y: -120,
                width: 1920,
                height: 1080,
            },
        ),
    ];

    assert_eq!(
        project_native_cursor_logical_viewport(
            sophia_protocol::Point { x: 20.75, y: -99.2 },
            &viewports,
        )
        .unwrap(),
        Some((
            raised,
            20,
            20,
            Size {
                width: 1920,
                height: 1080,
            },
        ))
    );
}

#[cfg(feature = "gbm-probe")]
#[test]
fn output_service_reduces_native_resource_state_without_handles() {
    assert_eq!(
        reduce_output_native_frame_phase(false, false),
        OutputNativeFramePhase::Idle
    );
    assert_eq!(
        reduce_output_native_frame_phase(true, false),
        OutputNativeFramePhase::InFlight
    );
    assert_eq!(
        reduce_output_native_frame_phase(false, true),
        OutputNativeFramePhase::CleanupPending
    );
    assert_eq!(
        reduce_output_native_frame_phase(true, true),
        OutputNativeFramePhase::CleanupPending
    );
}

impl FakeRenderedScanoutExporter {
    fn exported(size: Size) -> Self {
        Self {
            status: LiveRendererScanoutBufferExportStatus::Exported,
            descriptor: Some(scanout_descriptor(size)),
            owner: Some(FakeRenderedScanoutOwner { raw: 7 }),
            export_attempts: 0,
        }
    }

    fn unavailable() -> Self {
        Self {
            status: LiveRendererScanoutBufferExportStatus::Unavailable,
            descriptor: None,
            owner: None,
            export_attempts: 0,
        }
    }

    fn export_attempts(&self) -> usize {
        self.export_attempts
    }
}

impl LiveRenderedScanoutBufferExporter for FakeRenderedScanoutExporter {
    type Owner = FakeRenderedScanoutOwner;

    fn export_rendered_scanout_buffer(
        &mut self,
        _target: LiveGbmEglFrameTargetRecord,
    ) -> LiveRenderedScanoutBufferExport<Self::Owner> {
        self.export_attempts = self.export_attempts.saturating_add(1);
        LiveRenderedScanoutBufferExport {
            status: self.status,
            detail: LiveRendererScanoutBufferExportDetail::from_status(self.status),
            descriptor: self.descriptor,
            owner: self.owner.take(),
            correlation: None,
        }
    }
}

/// A cursor plane offers the ten properties a placement needs.
#[test]
fn cursor_plane_properties_are_discovered_when_the_plane_can_be_positioned() {
    use sophia_backend_live::discover_cursor_plane_properties;

    let device = full_property_lookup_device();
    assert!(
        discover_cursor_plane_properties(&device, plane_handle()).is_some(),
        "a plane carrying every positioning property is usable as a cursor"
    );
}

/// A plane missing any one of them is not a cursor plane this compositor can
/// drive, and that is an ordinary answer: the head keeps the legacy ioctl
/// rather than the session refusing to start over a plane it never needed.
#[test]
fn a_plane_that_cannot_be_positioned_is_not_offered_as_a_cursor() {
    use sophia_backend_live::discover_cursor_plane_properties;

    for missing in [
        "FB_ID", "CRTC_ID", "SRC_X", "SRC_Y", "SRC_W", "SRC_H", "CRTC_X", "CRTC_Y", "CRTC_W",
        "CRTC_H",
    ] {
        let mut device = full_property_lookup_device();
        let kept = [
            ("FB_ID", property_handle(104)),
            ("CRTC_ID", property_handle(105)),
            ("SRC_X", property_handle(106)),
            ("SRC_Y", property_handle(107)),
            ("SRC_W", property_handle(108)),
            ("SRC_H", property_handle(109)),
            ("CRTC_X", property_handle(110)),
            ("CRTC_Y", property_handle(111)),
            ("CRTC_W", property_handle(112)),
            ("CRTC_H", property_handle(113)),
        ]
        .into_iter()
        .filter(|(name, _)| *name != missing)
        .collect::<Vec<_>>();
        device.plane = Ok(LibdrmNativePropertyHandleSet::new(kept));
        assert!(
            discover_cursor_plane_properties(&device, plane_handle()).is_none(),
            "a plane without {missing} cannot carry a cursor"
        );
    }
}

/// A read failure is not a usable cursor plane either.
#[test]
fn a_plane_whose_properties_cannot_be_read_is_not_offered_as_a_cursor() {
    use sophia_backend_live::discover_cursor_plane_properties;

    let mut device = full_property_lookup_device();
    device.plane = Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    assert!(discover_cursor_plane_properties(&device, plane_handle()).is_none());
}
