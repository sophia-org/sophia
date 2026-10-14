//! A trusted adapter publishes completed presentations here. Ordinary frames
//! replace the candidate, not a recipient's already selected immutable image.
use crate::{capture_frame::PortalCaptureFrame, coordinator::*};
use sophia_protocol::{
    OutputId, PortalGrant, PortalRequest, PortalTransferId, PortalTransferKind,
    output_capture::{OUTPUT_CAPTURE_MIME, OUTPUT_CAPTURE_RECORD_LEN},
};
use std::sync::{Arc, Mutex};

struct State {
    generation: u64,
    available: bool,
    current: Option<Arc<PortalCaptureFrame>>,
    selected: Option<(PortalTransferId, Arc<PortalCaptureFrame>)>,
}

pub struct OutputCaptureSource {
    output: OutputId,
    state: Mutex<State>,
}
impl OutputCaptureSource {
    /// One source belongs to one trusted recipient/coordinator binding. The
    /// common portal policy still owns source and target namespace admission.
    pub fn new(output: OutputId, generation: u64) -> Option<Self> {
        if !output.is_valid() || generation == 0 {
            return None;
        }
        Some(Self {
            output,
            state: Mutex::new(State {
                generation,
                available: true,
                current: None,
                selected: None,
            }),
        })
    }
    pub fn publish(&self, frame: Arc<PortalCaptureFrame>) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let info = frame.info();
        if !state.available
            || info.output != self.output
            || info.output_generation != state.generation
            || state.current.as_ref().is_some_and(|old| {
                old.info().presentation_generation >= info.presentation_generation
            })
        {
            return false;
        }
        state.current = Some(frame);
        true
    }
    /// Returns after any earlier guarded read. Withdrawal never revives an old
    /// generation; even the same numeric output requires a newer incarnation.
    pub fn withdraw(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.available = false;
            state.current = None;
            state.selected = None;
        }
    }
    pub fn replace(&self, generation: u64) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if generation <= state.generation {
            return false;
        }
        *state = State {
            generation,
            available: true,
            current: None,
            selected: None,
        };
        true
    }
    fn accepts(request: &PortalRequest) -> bool {
        request.transfer.kind == PortalTransferKind::ScreenCapture
            && request.transfer.mime_type.as_deref() == Some(OUTPUT_CAPTURE_MIME)
            && request.transfer.byte_size == OUTPUT_CAPTURE_RECORD_LEN as u64
    }
}
impl PortalSource for OutputCaptureSource {
    fn generation(&self, request: &PortalRequest) -> Option<u64> {
        if !Self::accepts(request) {
            return None;
        }
        let state = self.state.lock().ok()?;
        (state.available && state.current.is_some()).then_some(state.generation)
    }
    fn with_generation(
        &self,
        request: &PortalRequest,
        generation: u64,
        effect: &mut dyn FnMut(),
    ) -> bool {
        if !Self::accepts(request) {
            return false;
        }
        let Ok(state) = self.state.lock() else {
            return false;
        };
        if !state.available || state.generation != generation {
            return false;
        }
        effect();
        drop(state);
        true
    }
    fn collect(
        &self,
        request: &PortalRequest,
        grant: &PortalGrant,
        wait: &PortalCollection,
    ) -> Result<Vec<u8>, PortalTransferFailure> {
        wait.remaining()?;
        if !Self::accepts(request) {
            return Err(PortalTransferFailure::Source);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| PortalTransferFailure::Source)?;
        if !state.available || state.generation != grant.source_generation {
            return Err(PortalTransferFailure::Source);
        }
        let frame = state.current.clone().ok_or(PortalTransferFailure::Source)?;
        let record = frame
            .info()
            .encode()
            .ok_or(PortalTransferFailure::Source)?
            .to_vec();
        state.selected = Some((request.transfer.transfer, frame));
        Ok(record)
    }
    fn capture_frame(
        &self,
        request: &PortalRequest,
        grant: &PortalGrant,
    ) -> Option<Arc<PortalCaptureFrame>> {
        if !Self::accepts(request) {
            return None;
        }
        let mut state = self.state.lock().ok()?;
        if !state.available || state.generation != grant.source_generation {
            return None;
        }
        let (transfer, frame) = state.selected.as_ref()?;
        if *transfer != request.transfer.transfer {
            return None;
        }
        let frame = frame.clone();
        state.selected = None;
        Some(frame)
    }
}
