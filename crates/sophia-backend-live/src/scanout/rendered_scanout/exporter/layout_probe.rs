use super::layout_candidate::RetainedLayoutCandidate;
use super::*;
use crate::{
    LibdrmNativePlaneFormatSnapshot, LibdrmNativePlaneFormatSupport,
    LiveRenderedPrimaryPlaneScanoutCleanup, LiveScanoutLayoutProbeReport,
    RenderDeviceDiscoveryBackend,
};
use sophia_renderer_live::LiveRendererScanoutBufferDescriptor;
use std::time::{Duration, Instant};

const PROBE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Default)]
pub(super) struct ExporterLayoutProbe {
    pub candidate: RetainedLayoutCandidate,
    pub cleanup: Option<LiveRenderedPrimaryPlaneScanoutCleanup<NativeGbmRenderedScanoutOwner>>,
    pub last_report: Option<LiveScanoutLayoutProbeReport>,
    formats: Option<LibdrmNativePlaneFormatSnapshot>,
    available: bool,
    last_capture: Option<Instant>,
    last_attempt: Option<Instant>,
    pub(super) last_framebuffer_report: Option<Instant>,
}

impl<R: RenderDeviceDiscoveryBackend> NativeGbmRenderedScanoutBufferDiscoveryExporter<R> {
    pub fn with_layout_probe_formats(mut self, formats: LibdrmNativePlaneFormatSnapshot) -> Self {
        self.invalidate_layout_probe();
        self.layout_probe.formats = Some(formats);
        self
    }

    /// The native owner grants this only for an ordinary, quiescent card turn.
    pub(crate) fn set_layout_probe_available(&mut self, available: bool) {
        self.layout_probe.available = available;
        if !available {
            self.invalidate_layout_probe();
        }
    }

    pub(crate) fn invalidate_layout_probe(&mut self) {
        self.layout_probe.candidate.invalidate();
        self.layout_probe.last_report = None;
    }

    pub(crate) fn expire_layout_probe(&mut self, now: Instant) {
        self.layout_probe.candidate.expire(now);
    }

    pub fn layout_probe_cleanup_pending(&self) -> bool {
        self.layout_probe.cleanup.is_some()
    }

    pub fn last_layout_probe_report(&self) -> Option<LiveScanoutLayoutProbeReport> {
        self.layout_probe.last_report
    }

    pub(super) fn capture_layout_probe_source(
        &mut self,
        expected: Option<LiveRendererScanoutBufferDescriptor>,
    ) -> bool {
        let now = Instant::now();
        if !self.layout_probe.available
            || self.layout_probe.cleanup.is_some()
            || self
                .layout_probe
                .last_capture
                .is_some_and(|last| now.duration_since(last) < PROBE_INTERVAL)
        {
            return false;
        }
        let (Some((frame, native)), Some(target), Some(formats)) = (
            &self.direct_fallback,
            self.last_target,
            &self.layout_probe.formats,
        ) else {
            return false;
        };
        let Ok(buffer) = frame.direct_scanout_buffer(target.size) else {
            return false;
        };
        let descriptor = buffer.descriptor;
        if expected.is_some_and(|expected| expected != descriptor) {
            return false;
        }
        if layout_support(formats, descriptor) != LibdrmNativePlaneFormatSupport::Unsupported {
            return false;
        }
        let Ok(format) = drm::buffer::DrmFourcc::try_from(descriptor.format) else {
            return false;
        };
        if formats.modifiers(format).is_none_or(|rows| rows.is_empty()) {
            return false;
        }
        let original = LiveRendererFrameCorrelation {
            native: *native,
            request: None,
            trace: frame.trace,
            direct_scanout: Some(frame.direct_scanout),
        };
        if self
            .layout_probe
            .candidate
            .capture(buffer, original, now, now + PROBE_INTERVAL)
        {
            self.layout_probe.last_capture = Some(now);
            return true;
        }
        false
    }

    pub(super) fn take_layout_source(
        &mut self,
        completed: Option<LiveRendererFrameCorrelation>,
        descriptor: LiveRendererScanoutBufferDescriptor,
    ) -> Option<LiveScanoutLayoutProbeSource<NativeGbmRenderedScanoutOwner>> {
        if !self.layout_probe.available {
            self.invalidate_layout_probe();
            return None;
        }
        let now = Instant::now();
        if self
            .layout_probe
            .last_attempt
            .is_some_and(|last| now.duration_since(last) < PROBE_INTERVAL)
        {
            self.layout_probe.candidate.invalidate();
            return None;
        }
        let source = self
            .layout_probe
            .candidate
            .take(completed?, descriptor, now)?;
        let formats = self.layout_probe.formats.as_ref()?;
        if layout_support(formats, descriptor) != LibdrmNativePlaneFormatSupport::Supported
            || layout_support(formats, source.buffer.descriptor)
                != LibdrmNativePlaneFormatSupport::Unsupported
        {
            return None;
        }
        self.layout_probe.last_attempt = Some(now);
        Some(LiveScanoutLayoutProbeSource {
            image: source.buffer.image_id,
            export: LiveRenderedScanoutBufferExport::new(
                sophia_renderer_live::LiveRendererScanoutBufferExportStatus::Exported,
                sophia_renderer_live::LiveRendererScanoutBufferExportDetail::Exported,
                Some(source.buffer.descriptor),
                Some(NativeGbmRenderedScanoutOwner::Direct(source.buffer)),
            )
            .with_correlation(Some(source.original)),
        })
    }
}

fn layout_support(
    formats: &LibdrmNativePlaneFormatSnapshot,
    descriptor: LiveRendererScanoutBufferDescriptor,
) -> LibdrmNativePlaneFormatSupport {
    let (Ok(format), Some(modifier)) = (
        drm::buffer::DrmFourcc::try_from(descriptor.format),
        descriptor.modifier,
    ) else {
        return LibdrmNativePlaneFormatSupport::Unknown;
    };
    if modifier == u64::MAX {
        return LibdrmNativePlaneFormatSupport::Unknown;
    }
    formats.support(format, drm::buffer::DrmModifier::from(modifier))
}

#[path = "../../../../tests/support/exporter_layout_probe.rs"]
mod tests;
