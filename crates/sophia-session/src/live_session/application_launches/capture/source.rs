//! Couples the portal source to native withdrawal without a Session poll.
use super::*;
use sophia_backend_live::LiveOutputCaptureValidity;
use sophia_portal::coordinator::{PortalCollection, PortalSource};

pub(super) struct Source {
    pub image: OutputCaptureSource,
    pub validity: LiveOutputCaptureValidity,
}
impl Source {
    pub fn withdraw(&self) {
        self.image.withdraw();
    }
}
impl PortalSource for Source {
    fn generation(&self, request: &PortalRequest) -> Option<u64> {
        self.validity
            .with_current(|| self.image.generation(request))
            .flatten()
    }
    fn with_generation(
        &self,
        request: &PortalRequest,
        generation: u64,
        effect: &mut dyn FnMut(),
    ) -> bool {
        self.validity
            .with_current(|| self.image.with_generation(request, generation, effect))
            .unwrap_or(false)
    }
    fn collect(
        &self,
        request: &PortalRequest,
        grant: &PortalGrant,
        wait: &PortalCollection,
    ) -> Result<Vec<u8>, PortalTransferFailure> {
        self.validity
            .with_current(|| ())
            .ok_or(PortalTransferFailure::Source)?;
        // Collection checks the execution fence, so it must not hold the
        // native guard in reverse lock order. This only stages a record;
        // installation and every delivery recheck native validity.
        self.image.collect(request, grant, wait)
    }
    fn capture_frame(
        &self,
        request: &PortalRequest,
        grant: &PortalGrant,
    ) -> Option<Arc<sophia_portal::capture_frame::PortalCaptureFrame>> {
        self.validity
            .with_current(|| self.image.capture_frame(request, grant))
            .flatten()
    }
}
