#[cfg(feature = "gbm-probe")]
use super::RealAtomicScanoutRenderDeviceDiscovery;
use super::{
    RealAtomicScanoutCard, RealAtomicScanoutCardSelection, RealAtomicScanoutCardSelectionStatus,
};
use crate::prelude::*;

#[cfg(all(feature = "gbm-probe", feature = "libdrm-events"))]
mod format_capabilities;

#[derive(Debug)]
pub struct RealAtomicScanoutPageFlipSession {
    pub(super) card: RealAtomicScanoutCard,
    selections: Vec<LibdrmNativePrimaryPlaneSelection>,
    outputs: Vec<OutputId>,
    heads: Vec<sophia_engine::RenderHeadId>,
    pub(super) reader: NativeLibdrmPageFlipEventReader<RealAtomicScanoutCard>,
    pub(super) poller: NativeLibdrmPageFlipEventPoller,
    #[cfg(feature = "gbm-probe")]
    cursor_buffer: Option<drm::control::dumbbuffer::DumbBuffer>,
    #[cfg(feature = "gbm-probe")]
    cursor_framebuffer: Option<drm::control::framebuffer::Handle>,
    #[cfg(feature = "gbm-probe")]
    cursor_plane_probe: Option<crate::CursorPlaneProbe>,
    #[cfg(feature = "gbm-probe")]
    cursor_dimensions: Option<LegacyHardwareCursorDimensions>,
    #[cfg(feature = "gbm-probe")]
    cursor_planes: Option<Vec<RealAtomicCursorPlane>>,
    #[cfg(feature = "gbm-probe")]
    cursor_controller: LegacyHardwareCursorController<drm::control::crtc::Handle>,
    #[cfg(feature = "gbm-probe")]
    cursor_crtcs_sanitized: bool,
    #[cfg(feature = "gbm-probe")]
    cursor_asset: sophia_engine::CursorAsset,
}

#[cfg(feature = "gbm-probe")]
#[derive(Clone, Debug)]
struct RealAtomicCursorPlane {
    plane: drm::control::plane::Handle,
    fb_id: drm::control::property::Handle,
    crtc_id: drm::control::property::Handle,
}

#[cfg(feature = "gbm-probe")]
fn cursor_raster_fits(
    dimensions: LegacyHardwareCursorDimensions,
    asset: &sophia_engine::CursorAsset,
) -> io::Result<()> {
    if dimensions.width < asset.width() || dimensions.height < asset.height() {
        return Err(io::Error::other(format!(
            "driver cursor dimensions {}x{} are smaller than the {}x{} cursor raster",
            dimensions.width,
            dimensions.height,
            asset.width(),
            asset.height(),
        )));
    }
    Ok(())
}

/// Writes an asset's pixels into the cursor buffer, clearing what was there.
///
/// The buffer is the size the driver reports for a cursor, not the size of the
/// raster, so a smaller asset leaves the rest transparent and a replacement
/// never needs a new buffer. That is what makes swapping the cursor at runtime
/// a fill rather than a reallocation: the plane keeps scanning out the same
/// handle and only its contents change.
#[cfg(feature = "gbm-probe")]
fn paint_hardware_cursor(
    card: &RealAtomicScanoutCard,
    buffer: &mut drm::control::dumbbuffer::DumbBuffer,
    asset: &sophia_engine::CursorAsset,
) -> io::Result<()> {
    let pitch = usize::try_from(drm::buffer::Buffer::pitch(buffer))
        .map_err(|_| io::Error::other("cursor pitch exceeds address space"))?;
    let raster_stride = usize::try_from(asset.width())
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| io::Error::other("cursor raster stride overflow"))?;
    use drm::control::Device as _;

    let mut mapping = card.map_dumb_buffer(buffer)?;
    mapping.fill(0);
    for (y, row) in asset.pixels().chunks_exact(raster_stride).enumerate() {
        let offset = y * pitch;
        mapping[offset..offset + raster_stride].copy_from_slice(row);
    }
    Ok(())
}

#[cfg(feature = "gbm-probe")]
struct RealLegacyHardwareCursorDevice<'a> {
    card: &'a RealAtomicScanoutCard,
    buffer: &'a drm::control::dumbbuffer::DumbBuffer,
}

#[cfg(feature = "gbm-probe")]
impl LegacyHardwareCursorDevice for RealLegacyHardwareCursorDevice<'_> {
    type Crtc = drm::control::crtc::Handle;

    #[allow(deprecated)]
    fn hide_cursor(&mut self, crtc: Self::Crtc) -> io::Result<()> {
        use drm::control::Device as _;

        self.card
            .set_cursor::<drm::control::dumbbuffer::DumbBuffer>(crtc, None)
    }

    #[allow(deprecated)]
    fn install_cursor(&mut self, crtc: Self::Crtc) -> io::Result<()> {
        use drm::control::Device as _;

        self.card.set_cursor2(crtc, Some(self.buffer), (0, 0))
    }

    #[allow(deprecated)]
    fn move_cursor(&mut self, crtc: Self::Crtc, x: i32, y: i32) -> io::Result<()> {
        use drm::control::Device as _;

        self.card.move_cursor(crtc, (x, y))
    }
}

impl RealAtomicScanoutPageFlipSession {
    /// Installs the cursor the hardware plane scans out, before or after it
    /// has a buffer.
    ///
    /// This used to refuse once a buffer existed, which made the cursor a
    /// startup-only decision and left the two backends disagreeing -- the CPU
    /// scene could already swap its asset. Nothing about the hardware required
    /// that: the buffer is sized to the driver's cursor dimensions rather than
    /// to the raster, so a replacement repaints the buffer the plane is
    /// already scanning and neither the buffer nor the framebuffer handle
    /// changes. There is no commit to sequence and nothing to roll back.
    ///
    /// A raster the driver cannot hold is refused with the buffer untouched,
    /// so a caller that asked for a size this card will not take still has the
    /// cursor it had.
    #[cfg(feature = "gbm-probe")]
    pub fn set_hardware_cursor_asset(
        &mut self,
        asset: sophia_engine::CursorAsset,
    ) -> io::Result<()> {
        if let Some(dimensions) = self.cursor_dimensions {
            cursor_raster_fits(dimensions, &asset)?;
        }
        if let Some(buffer) = self.cursor_buffer.as_mut() {
            paint_hardware_cursor(&self.card, buffer, &asset)?;
        }
        self.cursor_asset = asset;
        Ok(())
    }

    /// Whether a raster of this size could be installed on this card.
    ///
    /// Asked before rendering one, because producing a cursor the driver will
    /// not take costs a raster and tells the caller nothing it can act on. The
    /// dimensions are unknown until the first cursor is prepared, and an
    /// unknown answer is optimistic: refusing on ignorance would disable the
    /// feature on every card before it was ever asked.
    #[cfg(feature = "gbm-probe")]
    pub fn hardware_cursor_admits_size(&self, width: u32, height: u32) -> bool {
        self.cursor_dimensions
            .is_none_or(|dimensions| dimensions.width >= width && dimensions.height >= height)
    }

    #[cfg(feature = "gbm-probe")]
    pub const fn hardware_cursor_hotspot(&self) -> (u32, u32) {
        self.cursor_asset.hotspot()
    }

    #[cfg(feature = "gbm-probe")]
    fn discover_atomic_cursor_planes(&self) -> io::Result<Vec<RealAtomicCursorPlane>> {
        let mut cursor_planes = Vec::new();
        for plane in LibdrmNativeKmsSelectionDevice::plane_handles(&self.card)? {
            if LibdrmNativeKmsSelectionDevice::plane_type(&self.card, plane)?
                != Some(drm::control::PlaneType::Cursor)
            {
                continue;
            }
            let snapshot = LibdrmNativeKmsSelectionDevice::plane_snapshot(&self.card, plane)?;
            let crtcs = self
                .selections
                .iter()
                .filter_map(|selection| {
                    snapshot
                        .supports_crtc(selection.crtc)
                        .then_some(selection.crtc)
                })
                .collect::<Vec<_>>();
            if crtcs.is_empty() {
                continue;
            }
            let properties =
                LibdrmNativePropertyLookupDevice::plane_property_handles(&self.card, plane)?;
            let required = |name| {
                properties.get(name).ok_or_else(|| {
                    io::Error::other(format!("atomic cursor plane is missing {name}"))
                })
            };
            cursor_planes.push(RealAtomicCursorPlane {
                plane,
                fb_id: required("FB_ID")?,
                crtc_id: required("CRTC_ID")?,
            });
        }
        Ok(cursor_planes)
    }

    #[cfg(feature = "gbm-probe")]
    /// A framebuffer over the cursor buffer, for a cursor plane to point at.
    ///
    /// The legacy path hands the dumb buffer straight to `set_cursor2`; an
    /// atomic request needs a framebuffer handle instead. Same pixels, same
    /// buffer -- only the way the plane is told about them differs, which is
    /// why this is created beside the buffer rather than duplicating it.
    #[cfg(feature = "gbm-probe")]
    fn ensure_atomic_cursor_framebuffer(
        &mut self,
    ) -> io::Result<drm::control::framebuffer::Handle> {
        use drm::control::Device as _;

        if let Some(framebuffer) = self.cursor_framebuffer {
            return Ok(framebuffer);
        }
        let buffer = self
            .cursor_buffer
            .as_ref()
            .ok_or_else(|| io::Error::other("cursor buffer is unavailable"))?;
        let framebuffer = self.card.add_framebuffer(buffer, 32, 32)?;
        self.cursor_framebuffer = Some(framebuffer);
        Ok(framebuffer)
    }

    /// Ask the driver once whether it will scan this cursor plane.
    ///
    /// A test commit, never a real one: the answer is about format and size,
    /// which do not change after this, and paying for a test on every frame
    /// would buy nothing. A refusal keeps the legacy path, which works.
    #[cfg(feature = "gbm-probe")]
    fn probe_atomic_cursor_plane(
        &mut self,
        cursor: &RealAtomicCursorPlane,
        crtc: drm::control::crtc::Handle,
    ) -> io::Result<crate::CursorPlaneProbe> {
        use drm::control::Device as _;

        let framebuffer = self.ensure_atomic_cursor_framebuffer()?;
        let dimensions = self
            .cursor_dimensions
            .ok_or_else(|| io::Error::other("hardware cursor dimensions are unavailable"))?;
        let Some(properties) = crate::discover_cursor_plane_properties(&self.card, cursor.plane)
        else {
            return Ok(crate::CursorPlaneProbe::Refused);
        };
        let mut request = drm::control::atomic::AtomicModeReq::new();
        crate::add_cursor_plane_properties(
            &mut request,
            cursor.plane,
            crtc,
            properties,
            Some(crate::LibdrmNativeCursorPlacement {
                framebuffer,
                x: 0,
                y: 0,
                width: dimensions.width,
                height: dimensions.height,
            }),
        );
        match self
            .card
            .atomic_commit(drm::control::AtomicCommitFlags::TEST_ONLY, request)
        {
            Ok(()) => Ok(crate::CursorPlaneProbe::Accepted),
            Err(_) => Ok(crate::CursorPlaneProbe::Refused),
        }
    }

    /// Probe the cursor plane of the first head that has one.
    ///
    /// One answer for the card, because the buffer and its format are the
    /// card's, not the head's. A card whose heads disagree would be a
    /// surprise worth finding on hardware rather than a case to invent here.
    #[cfg(feature = "gbm-probe")]
    fn probe_first_atomic_cursor_plane(&mut self) -> io::Result<crate::CursorPlaneProbe> {
        let Some((cursor, crtc)) = self.cursor_planes.as_ref().and_then(|planes| {
            planes
                .first()
                .cloned()
                .zip(self.selections.first().map(|selection| selection.crtc))
        }) else {
            return Ok(crate::CursorPlaneProbe::Refused);
        };
        self.probe_atomic_cursor_plane(&cursor, crtc)
    }

    /// The framebuffer and size a cursor plane needs, once prepared.
    ///
    /// One call rather than three accessors: a caller that got the buffer
    /// without the dimensions could commit a cursor sized for a different
    /// card, and there is no reason to make that expressible.
    #[cfg(feature = "gbm-probe")]
    pub fn atomic_cursor_resources(&self) -> Option<(drm::control::framebuffer::Handle, u32, u32)> {
        let framebuffer = self.cursor_framebuffer?;
        let dimensions = self.cursor_dimensions?;
        Some((framebuffer, dimensions.width, dimensions.height))
    }

    /// The card, for a caller committing a cursor on one of its planes.
    #[cfg(feature = "gbm-probe")]
    pub const fn cursor_commit_device(&self) -> &RealAtomicScanoutCard {
        &self.card
    }

    /// What the startup probe concluded, once it has run.
    #[cfg(feature = "gbm-probe")]
    pub const fn cursor_plane_probe(&self) -> Option<crate::CursorPlaneProbe> {
        self.cursor_plane_probe
    }

    #[cfg(feature = "gbm-probe")]
    fn detach_atomic_cursor_planes(&self, planes: &[RealAtomicCursorPlane]) -> io::Result<()> {
        use drm::control::Device as _;

        if planes.is_empty() {
            return Ok(());
        }
        let mut request = drm::control::atomic::AtomicModeReq::new();
        for cursor in planes {
            request.add_property(
                cursor.plane,
                cursor.fb_id,
                drm::control::property::Value::Framebuffer(None),
            );
            request.add_property(
                cursor.plane,
                cursor.crtc_id,
                drm::control::property::Value::CRTC(None),
            );
        }
        self.card
            .atomic_commit(drm::control::AtomicCommitFlags::empty(), request)
    }

    #[cfg(feature = "gbm-probe")]
    pub fn classic_hardware_cursor_initialized(&self) -> bool {
        self.cursor_controller.is_initialized()
    }

    #[cfg(feature = "gbm-probe")]
    /// The cursor's planes, dimensions, and pixels -- everything both paths
    /// need before either can show one.
    ///
    /// Split out of legacy initialization because the probe used to live
    /// inside it, which meant the capability check would have stopped running
    /// the moment a session drove the atomic path instead: it would have
    /// disappeared exactly when it started mattering.
    #[cfg(feature = "gbm-probe")]
    pub fn prepare_hardware_cursor(&mut self) -> io::Result<()> {
        use drm::{Device as _, control::Device as _};

        if self.cursor_planes.is_none() {
            self.cursor_planes = Some(self.discover_atomic_cursor_planes()?);
        }
        if !self.cursor_crtcs_sanitized {
            let planes = self
                .cursor_planes
                .as_deref()
                .ok_or_else(|| io::Error::other("atomic cursor planes disappeared"))?;
            self.detach_atomic_cursor_planes(planes)?;
            self.cursor_crtcs_sanitized = true;
        }
        if self.cursor_dimensions.is_none() {
            let width = self
                .card
                .get_driver_capability(drm::DriverCapability::CursorWidth)
                .ok();
            let height = self
                .card
                .get_driver_capability(drm::DriverCapability::CursorHeight)
                .ok();
            self.cursor_dimensions = Some(resolve_legacy_hardware_cursor_dimensions(width, height));
        }
        if self.cursor_buffer.is_none() {
            let dimensions = self
                .cursor_dimensions
                .ok_or_else(|| io::Error::other("hardware cursor dimensions are unavailable"))?;
            cursor_raster_fits(dimensions, &self.cursor_asset)?;
            let mut buffer = self.card.create_dumb_buffer(
                (dimensions.width, dimensions.height),
                drm::buffer::DrmFourcc::Argb8888,
                32,
            )?;
            paint_hardware_cursor(&self.card, &mut buffer, &self.cursor_asset)?;
            self.cursor_buffer = Some(buffer);
        }
        // Ask the card once whether it would scan a cursor plane, and record
        // the answer without acting on it. The commit path is not written
        // yet, so the cursor still rides the legacy ioctl -- reporting the
        // capability is not the same as claiming to use it, and evidence
        // that conflated the two would be worse than no evidence.
        if self.cursor_plane_probe.is_none() {
            let probe = self.probe_first_atomic_cursor_plane()?;
            tracing::info!(
                "sophia_live_cursor_plane schema=1 status={}",
                match probe {
                    crate::CursorPlaneProbe::Accepted => "accepted",
                    crate::CursorPlaneProbe::Refused => "refused",
                }
            );
            self.cursor_plane_probe = Some(probe);
        }
        Ok(())
    }

    #[cfg(feature = "gbm-probe")]
    pub fn initialize_classic_hardware_cursor(&mut self) -> io::Result<()> {
        if self.cursor_controller.is_initialized() {
            return Ok(());
        }
        self.prepare_hardware_cursor()?;

        let crtcs = self
            .selections
            .iter()
            .map(|selection| selection.crtc)
            .collect::<Vec<_>>();
        let buffer = self
            .cursor_buffer
            .as_ref()
            .ok_or_else(|| io::Error::other("legacy hardware cursor buffer is unavailable"))?;
        let mut device = RealLegacyHardwareCursorDevice {
            card: &self.card,
            buffer,
        };
        self.cursor_controller.initialize(&mut device, &crtcs)
    }

    #[cfg(feature = "gbm-probe")]
    pub fn update_classic_hardware_cursor(
        &mut self,
        target: Option<(LibdrmNativePrimaryPlaneSelection, i32, i32)>,
    ) -> io::Result<ClassicHardwareCursorUpdate> {
        let target = target
            .filter(|(selection, _, _)| {
                self.selections
                    .iter()
                    .any(|candidate| candidate.crtc == selection.crtc)
            })
            .map(|(selection, x, y)| LegacyHardwareCursorTarget {
                crtc: selection.crtc,
                x,
                y,
            });
        let buffer = self
            .cursor_buffer
            .as_ref()
            .ok_or_else(|| io::Error::other("legacy hardware cursor buffer is unavailable"))?;
        let mut device = RealLegacyHardwareCursorDevice {
            card: &self.card,
            buffer,
        };
        self.cursor_controller.update(&mut device, target)
    }

    #[cfg(feature = "gbm-probe")]
    pub fn update_classic_hardware_cursors(
        &mut self,
        targets: &[(LibdrmNativePrimaryPlaneSelection, i32, i32)],
    ) -> io::Result<ClassicHardwareCursorUpdate> {
        let targets = targets
            .iter()
            .filter(|(selection, _, _)| {
                self.selections
                    .iter()
                    .any(|candidate| candidate.crtc == selection.crtc)
            })
            .map(|(selection, x, y)| LegacyHardwareCursorTarget {
                crtc: selection.crtc,
                x: *x,
                y: *y,
            })
            .collect::<Vec<_>>();
        let buffer = self
            .cursor_buffer
            .as_ref()
            .ok_or_else(|| io::Error::other("legacy hardware cursor buffer is unavailable"))?;
        let mut device = RealLegacyHardwareCursorDevice {
            card: &self.card,
            buffer,
        };
        self.cursor_controller.update_many(&mut device, &targets)
    }

    pub fn card(&self) -> &RealAtomicScanoutCard {
        &self.card
    }

    pub fn selection(&self) -> LibdrmNativePrimaryPlaneSelection {
        self.selections[0]
    }

    pub fn selections(&self) -> &[LibdrmNativePrimaryPlaneSelection] {
        &self.selections
    }

    pub fn heads(&self) -> &[sophia_engine::RenderHeadId] {
        &self.heads
    }

    pub fn outputs(&self) -> &[OutputId] {
        &self.outputs
    }

    pub fn output_capabilities(&self) -> io::Result<Vec<LibdrmNativeOutputCapability>> {
        self.selections
            .iter()
            .copied()
            .zip(self.outputs.iter().copied())
            .map(|(selection, output)| {
                read_native_output_capability(&self.card, selection, output)?
                    .with_gpu_identity(self.card.gpu_identity())
            })
            .collect()
    }

    pub fn vrr_properties_for_selection(
        &self,
        selection: LibdrmNativePrimaryPlaneSelection,
    ) -> LibdrmNativeVrrPropertyDiscoveryResult {
        discover_native_vrr_properties(&self.card, selection.connector, selection.crtc)
    }

    pub fn property_names_for_selection(
        &self,
        selection: LibdrmNativePrimaryPlaneSelection,
    ) -> io::Result<(Vec<String>, Vec<String>)> {
        let mut connector = self
            .card
            .connector_property_handles(selection.connector)?
            .names()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut crtc = self
            .card
            .crtc_property_handles(selection.crtc)?
            .names()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        connector.sort();
        crtc.sort();
        Ok((connector, crtc))
    }

    #[cfg(feature = "gbm-probe")]
    pub fn render_device_discovery(&self) -> io::Result<RealAtomicScanoutRenderDeviceDiscovery> {
        RealAtomicScanoutRenderDeviceDiscovery::from_card(&self.card)
    }

    #[cfg(all(feature = "gbm-probe", feature = "libinput-events"))]
    pub fn run_tick_with_native_gbm_rendered_primary_plane_scanout<P, E>(
        &mut self,
        runtime: &mut LiveBackendRuntimeAssembly<LiveInputReadinessGatedPoller<P>>,
        input: CompositorBackendTickInput,
        readiness: LiveBackendSessionLoopReadiness,
        page_flip_budget: LiveBackendSessionLoopPageFlipBudget,
        exporter: &mut NativeGbmRenderedScanoutBufferDiscoveryExporter<E>,
        sender: &std::sync::mpsc::SyncSender<LivePageFlipCallback>,
    ) -> Result<LiveBackendSessionLoopTickReport, CompositorBackendAssemblyError>
    where
        P: NonBlockingInputPoller,
        E: RenderDeviceDiscoveryBackend,
    {
        runtime.run_session_loop_tick_with_native_gbm_rendered_primary_plane_scanout_exporter_and_native_page_flip_events_with(
            input,
            readiness,
            page_flip_budget,
            &self.card,
            exporter,
            &mut self.reader,
            &mut self.poller,
            sender,
        )
    }

    #[cfg(all(feature = "gbm-probe", feature = "libdrm-events"))]
    #[allow(clippy::too_many_arguments)]
    pub fn run_native_gbm_runtime_tick<P, E>(
        &mut self,
        runtime: &mut LiveBackendRuntimeAssembly<P>,
        input: CompositorBackendTickInput,
        exporter: &mut NativeGbmRenderedScanoutBufferDiscoveryExporter<E>,
        sender: &std::sync::mpsc::SyncSender<LivePageFlipCallback>,
        max_read: usize,
        max_emit: usize,
    ) -> Result<LiveBackendRuntimeNativePageFlipTickReport, CompositorBackendAssemblyError>
    where
        P: NonBlockingInputPoller,
        E: RenderDeviceDiscoveryBackend,
    {
        runtime
            .run_tick_with_native_gbm_rendered_primary_plane_scanout_exporter_and_native_page_flip_events_with(
                input,
                &self.card,
                exporter,
                &mut self.reader,
                &mut self.poller,
                sender,
                max_read,
                max_emit,
            )
    }

    #[cfg(feature = "libdrm-events")]
    pub fn poll_native_page_flip_events(
        &mut self,
        sender: &std::sync::mpsc::SyncSender<LivePageFlipCallback>,
        max_read: usize,
        max_emit: usize,
    ) -> LibdrmNativeReadAndPollReport {
        self.poller
            .read_and_poll_page_flip_events(&mut self.reader, sender, max_read, max_emit)
    }

    #[cfg(feature = "libdrm-events")]
    pub fn collect_native_page_flip_events(
        &mut self,
        callbacks: &mut Vec<LivePageFlipCallback>,
        timestamps: &mut Vec<LibdrmKernelPageFlipTimestamp>,
        max_read: usize,
        max_emit: usize,
    ) -> LibdrmNativeReadAndPollReport {
        self.poller.read_and_collect_page_flip_events(
            &mut self.reader,
            callbacks,
            timestamps,
            max_read,
            max_emit,
        )
    }

    #[cfg(feature = "libdrm-events")]
    pub fn drain_emitted_kernel_page_flip_timestamps(
        &mut self,
    ) -> Vec<LibdrmKernelPageFlipTimestamp> {
        self.poller.drain_emitted_kernel_timestamps()
    }

    /// What the event poller holds and last saw, for stall attribution.
    ///
    /// A hard stall has two possible authors: an event the kernel never
    /// delivered, and an event delivered but stuck or dropped on the way
    /// through. The poller's pending depth, route count, and last read status
    /// are what separate them at the moment the stall is declared.
    #[cfg(feature = "libdrm-events")]
    pub fn page_flip_poller_diagnostics(&self) -> LibdrmNativePollerDiagnostics {
        self.poller.diagnostics()
    }

    #[cfg(feature = "libdrm-events")]
    pub fn page_flip_poller_cumulative_diagnostics(
        &self,
    ) -> LibdrmNativePollerCumulativeDiagnostics {
        self.poller.cumulative_diagnostics()
    }
}

impl Drop for RealAtomicScanoutPageFlipSession {
    fn drop(&mut self) {
        #[cfg(feature = "gbm-probe")]
        {
            use drm::control::Device as _;
            if let Some(buffer) = self.cursor_buffer.as_ref() {
                let mut device = RealLegacyHardwareCursorDevice {
                    card: &self.card,
                    buffer,
                };
                let _ = self.cursor_controller.hide_for_teardown(&mut device);
            }
            // Before the buffer it describes: a framebuffer outliving its
            // buffer is the leak the resource-bundle discipline exists to
            // prevent, and the cursor is no exception to it.
            if let Some(framebuffer) = self.cursor_framebuffer.take() {
                let _ = self.card.destroy_framebuffer(framebuffer);
            }
            if let Some(buffer) = self.cursor_buffer.take() {
                let _ = self.card.destroy_dumb_buffer(buffer);
            }
        }
    }
}

#[derive(Debug)]
pub struct RealAtomicScanoutPageFlipSessionResult {
    pub status: RealAtomicScanoutPageFlipSessionStatus,
    pub card_selection_status: RealAtomicScanoutCardSelectionStatus,
    pub session: Option<RealAtomicScanoutPageFlipSession>,
}

impl RealAtomicScanoutPageFlipSessionResult {
    pub fn failure_evidence(&self) -> Option<LibdrmNativeAtomicScanoutSmokeEvidence> {
        match self.status {
            RealAtomicScanoutPageFlipSessionStatus::Ready => None,
            RealAtomicScanoutPageFlipSessionStatus::CardSelectionFailed => {
                Some(self.card_selection_status.failure_evidence())
            }
            RealAtomicScanoutPageFlipSessionStatus::CardCloneFailed => {
                let mut evidence = LibdrmNativeAtomicScanoutSmokeEvidence::kms_selection_failed();
                evidence.status = LibdrmNativeAtomicScanoutSmokeStatus::PageFlipReaderUnavailable;
                Some(evidence)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealAtomicScanoutPageFlipSessionStatus {
    Ready,
    CardSelectionFailed,
    CardCloneFailed,
}

impl RealAtomicScanoutCardSelection {
    pub fn into_page_flip_session(
        mut self,
        slot: LibdrmNativeOutputSlot,
        output: OutputId,
        head: sophia_engine::RenderHeadId,
        authority: LibdrmBackendFdAuthority,
    ) -> RealAtomicScanoutPageFlipSessionResult {
        let Some(card) = self.card.take() else {
            return RealAtomicScanoutPageFlipSessionResult {
                status: RealAtomicScanoutPageFlipSessionStatus::CardSelectionFailed,
                card_selection_status: self.status,
                session: None,
            };
        };
        let Some(selection) = self.selection else {
            return RealAtomicScanoutPageFlipSessionResult {
                status: RealAtomicScanoutPageFlipSessionStatus::CardSelectionFailed,
                card_selection_status: self.status,
                session: None,
            };
        };
        if self.status != RealAtomicScanoutCardSelectionStatus::Selected {
            return RealAtomicScanoutPageFlipSessionResult {
                status: RealAtomicScanoutPageFlipSessionStatus::CardSelectionFailed,
                card_selection_status: self.status,
                session: None,
            };
        };

        let Ok(reader_card) = card.try_clone() else {
            return RealAtomicScanoutPageFlipSessionResult {
                status: RealAtomicScanoutPageFlipSessionStatus::CardCloneFailed,
                card_selection_status: self.status,
                session: None,
            };
        };
        let reader = NativeLibdrmPageFlipEventReader::new(reader_card)
            .with_crtc_routes([selection.crtc_route(slot)]);
        let poller = NativeLibdrmPageFlipEventPoller::new(
            LibdrmNativePageFlipSource::from_authority(authority),
        )
        .with_routes([LibdrmNativeOutputRoute { slot, output, head }]);

        RealAtomicScanoutPageFlipSessionResult {
            status: RealAtomicScanoutPageFlipSessionStatus::Ready,
            card_selection_status: self.status,
            session: Some(RealAtomicScanoutPageFlipSession {
                card,
                selections: vec![selection],
                outputs: vec![output],
                heads: vec![head],
                reader,
                poller,
                #[cfg(feature = "gbm-probe")]
                cursor_buffer: None,
                #[cfg(feature = "gbm-probe")]
                cursor_framebuffer: None,
                #[cfg(feature = "gbm-probe")]
                cursor_plane_probe: None,
                #[cfg(feature = "gbm-probe")]
                cursor_dimensions: None,
                #[cfg(feature = "gbm-probe")]
                cursor_planes: None,
                #[cfg(feature = "gbm-probe")]
                cursor_controller: LegacyHardwareCursorController::default(),
                #[cfg(feature = "gbm-probe")]
                cursor_crtcs_sanitized: false,
                #[cfg(feature = "gbm-probe")]
                cursor_asset: sophia_engine::x11_core_left_ptr_cursor(1),
            }),
        }
    }
}

#[derive(Debug)]
pub struct RealAtomicScanoutPageFlipSessionSetResult {
    pub status: RealAtomicScanoutPageFlipSessionSetStatus,
    pub sessions: Vec<RealAtomicScanoutPageFlipSession>,
    pub output_count: usize,
    /// The backend head table for the built sessions: which card, connector,
    /// and CRTC each minted head names. Physical identity stays here; the
    /// sessions and routes above carry only the opaque head.
    pub head_records: Vec<crate::LiveNativeHeadRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealAtomicScanoutPageFlipSessionSetStatus {
    Ready,
    SelectionFailed,
    CardCloneFailed,
    CapacityExceeded,
}

impl RealAtomicScanoutSelectionSet {
    pub fn into_page_flip_sessions(
        self,
        authority: LibdrmBackendFdAuthority,
    ) -> RealAtomicScanoutPageFlipSessionSetResult {
        self.into_page_flip_sessions_with_mirroring(authority, &NativeMirrorGrouping::none())
    }

    /// Builds sessions, giving every connector in a mirror group one logical output.
    ///
    /// Slots stay per connector because each head has its own page flip to intake;
    /// only the output identity is shared. That is what makes a mirror group one
    /// `SnapshotOutput` to policy while remaining N heads to the kernel.
    pub fn into_page_flip_sessions_with_mirroring(
        self,
        authority: LibdrmBackendFdAuthority,
        grouping: &NativeMirrorGrouping,
    ) -> RealAtomicScanoutPageFlipSessionSetResult {
        if self.status != RealAtomicScanoutSelectionSetStatus::SelectedAll {
            return RealAtomicScanoutPageFlipSessionSetResult {
                status: RealAtomicScanoutPageFlipSessionSetStatus::SelectionFailed,
                sessions: Vec::new(),
                output_count: 0,
                head_records: Vec::new(),
            };
        }
        let mut sessions = Vec::new();
        let mut head_records = Vec::new();
        let mut allocator = sophia_engine::RenderHeadAllocator::new();
        let mut next_output = 1u64;
        let mut next_slot = 1u16;
        // Logical outputs already handed to a mirror group, so its later members
        // join it instead of taking a fresh identity.
        let mut group_outputs: BTreeMap<usize, OutputId> = BTreeMap::new();
        for (card_index, target_set) in self.cards.into_iter().enumerate() {
            let Ok(reader_card) = target_set.card.try_clone() else {
                return RealAtomicScanoutPageFlipSessionSetResult {
                    status: RealAtomicScanoutPageFlipSessionSetStatus::CardCloneFailed,
                    sessions: Vec::new(),
                    output_count: 0,
                    head_records: Vec::new(),
                };
            };
            let mut crtc_routes = Vec::new();
            let mut output_routes = Vec::new();
            let mut outputs = Vec::new();
            let mut heads = Vec::new();
            for selection in target_set.selections.iter().copied() {
                let Some(slot) = LibdrmNativeOutputSlot::new(next_slot) else {
                    return RealAtomicScanoutPageFlipSessionSetResult {
                        status: RealAtomicScanoutPageFlipSessionSetStatus::CapacityExceeded,
                        sessions: Vec::new(),
                        output_count: 0,
                        head_records: Vec::new(),
                    };
                };
                // The card answers the connector's name directly, which is what
                // configuration named it. Resolving here rather than upstream is
                // what keeps the grouping free of the id-to-name circularity.
                let connector_name = drm::control::Device::get_connector(
                    &target_set.card,
                    selection.connector_handle(),
                    false,
                )
                .map(|connector| connector.to_string())
                .unwrap_or_default();
                let connector_key = target_set.card.connector_key(&connector_name);
                let group = grouping
                    .group_of(&connector_key)
                    .or_else(|| grouping.group_of(&connector_name));
                let output = match group.and_then(|group| group_outputs.get(&group).copied()) {
                    Some(output) => output,
                    None => {
                        let output = OutputId::from_raw(next_output);
                        next_output = next_output.saturating_add(1);
                        if let Some(group) = group {
                            group_outputs.insert(group, output);
                        }
                        output
                    }
                };
                let head = allocator.mint();
                crtc_routes.push(selection.crtc_route(slot));
                output_routes.push(LibdrmNativeOutputRoute { slot, output, head });
                outputs.push(output);
                heads.push(head);
                head_records.push(crate::LiveNativeHeadRecord {
                    head,
                    output,
                    card_index,
                    connector_id: selection.connector_id(),
                    crtc_id: selection.crtc_id(),
                    connector_name: connector_key,
                });
                next_slot = next_slot.saturating_add(1);
            }
            let reader =
                NativeLibdrmPageFlipEventReader::new(reader_card).with_crtc_routes(crtc_routes);
            let poller = NativeLibdrmPageFlipEventPoller::new(
                LibdrmNativePageFlipSource::from_authority(authority),
            )
            .with_routes(output_routes);
            sessions.push(RealAtomicScanoutPageFlipSession {
                card: target_set.card,
                selections: target_set.selections,
                outputs,
                heads,
                reader,
                poller,
                #[cfg(feature = "gbm-probe")]
                cursor_buffer: None,
                #[cfg(feature = "gbm-probe")]
                cursor_framebuffer: None,
                #[cfg(feature = "gbm-probe")]
                cursor_plane_probe: None,
                #[cfg(feature = "gbm-probe")]
                cursor_dimensions: None,
                #[cfg(feature = "gbm-probe")]
                cursor_planes: None,
                #[cfg(feature = "gbm-probe")]
                cursor_controller: LegacyHardwareCursorController::default(),
                #[cfg(feature = "gbm-probe")]
                cursor_crtcs_sanitized: false,
                #[cfg(feature = "gbm-probe")]
                cursor_asset: sophia_engine::x11_core_left_ptr_cursor(1),
            });
        }
        let output_count = usize::try_from(next_output.saturating_sub(1)).unwrap_or(usize::MAX);
        RealAtomicScanoutPageFlipSessionSetResult {
            status: RealAtomicScanoutPageFlipSessionSetStatus::Ready,
            sessions,
            output_count,
            head_records,
        }
    }
}
