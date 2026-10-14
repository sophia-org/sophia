//! Native facts and completed pixels supplied to the Session admission owner.
use super::*;

pub(super) struct PresentedCapture {
    pub validity: sophia_backend_live::LiveOutputCaptureValidity,
    pub owner: u64,
    pub output: OutputId,
    pub head: sophia_engine::RenderHeadId,
    pub target_generation: u64,
    pub frame: u64,
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u8]>,
}
pub(super) trait CaptureBackend {
    fn target(&self, output: OutputId) -> Option<LiveOutputCaptureTarget>;
    fn allowed(&self, output: OutputId, names: &[String]) -> bool;
    fn queue(&mut self, output: OutputId) -> Result<Option<u64>, Status>;
    fn retains(&self, output: OutputId, frame: u64) -> bool;
    fn presented(&mut self, output: OutputId, frame: u64) -> Option<PresentedCapture>;
}

pub(super) struct NativeCaptureBackend<'a> {
    pub native: Option<&'a mut LiveProductionNativeScanout>,
    pub runtime: Option<&'a mut LiveProductionVisualRuntime>,
    pub scene: &'a LiveProductionCpuScene,
}
impl CaptureBackend for NativeCaptureBackend<'_> {
    fn target(&self, output: OutputId) -> Option<LiveOutputCaptureTarget> {
        self.runtime.as_ref()?;
        self.native.as_ref()?.output_capture_target(output)
    }
    fn allowed(&self, output: OutputId, names: &[String]) -> bool {
        self.native
            .as_ref()
            .is_some_and(|native| native.output_capture_allowed(output, names))
    }
    fn queue(&mut self, output: OutputId) -> Result<Option<u64>, Status> {
        let native = self.native.as_deref_mut().ok_or(Status::Unavailable)?;
        let runtime = self.runtime.as_deref_mut().ok_or(Status::Unavailable)?;
        runtime
            .queue_output_capture(native, self.scene, output)
            .map(|frame| frame.map(|frame| frame.raw()))
            .map_err(|_| Status::Unavailable)
    }
    fn retains(&self, output: OutputId, frame: u64) -> bool {
        // The opaque native ID is not constructed from a requester value.
        // Compare against owner-held content to decide whether to retry.
        self.native.as_ref().is_some_and(|native| {
            native.head_indices(output).into_iter().any(|index| {
                let head = &native.heads[index];
                [
                    head.pending_content,
                    head.rendering_content,
                    head.submitted_content,
                    head.presented_content,
                ]
                .into_iter()
                .flatten()
                .any(|content| content.frame().raw() == frame)
            })
        })
    }
    fn presented(&mut self, output: OutputId, frame: u64) -> Option<PresentedCapture> {
        let capture = self
            .runtime
            .as_ref()?
            .presented_output_capture(self.native.as_ref()?, output)?;
        let identity = capture.identity();
        if identity.frame() != frame {
            return None;
        }
        Some(PresentedCapture {
            validity: self
                .native
                .as_deref_mut()?
                .output_capture_validity(output)?,
            owner: identity.owner(),
            output: identity.output(),
            head: identity.head(),
            target_generation: identity.target_generation(),
            frame,
            width: capture.image().width(),
            height: capture.image().height(),
            pixels: capture.image().rgba().to_vec().into(),
        })
    }
}
