//! Immutable pixels travel with the same owner as their native scanout frame.
use super::LiveNativeFrameIdentity;
use sophia_renderer_live::LiveCompositionSnapshot;

/// Pixels alone prove neither presentation nor permission to disclose them.
#[derive(Clone)]
pub struct LiveNativeFrameCapture {
    identity: LiveNativeFrameIdentity,
    image: LiveCompositionSnapshot,
}

impl std::fmt::Debug for LiveNativeFrameCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveNativeFrameCapture")
            .field("identity", &self.identity)
            .field("width", &self.image.width())
            .field("height", &self.image.height())
            .finish_non_exhaustive()
    }
}

impl LiveNativeFrameCapture {
    pub(super) fn new(identity: LiveNativeFrameIdentity, image: LiveCompositionSnapshot) -> Self {
        Self { identity, image }
    }
    pub fn identity(&self) -> LiveNativeFrameIdentity {
        self.identity
    }
    pub fn image(&self) -> &LiveCompositionSnapshot {
        &self.image
    }
}
