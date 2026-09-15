#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LiveRendererWorkerRequestId(pub(super) u64);

/// Identity captured before rendering; request numbers are scoped to their worker facade.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveRendererFrameCorrelation {
    pub native: Option<super::LiveNativeFrameIdentity>,
    pub request: Option<LiveRendererWorkerRequestId>,
    pub trace: Option<sophia_renderer_live::LiveCompositionTrace>,
    pub direct_scanout: Option<sophia_engine::DirectScanoutVerdict>,
}
