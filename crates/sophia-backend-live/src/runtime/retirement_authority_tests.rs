//! Production retirement/custody with fake framebuffer cleanup only. No KMS.
use super::*;
use std::{cell::Cell, num::NonZeroU32, rc::Rc};

#[derive(Debug)]
struct Owner(Rc<Cell<usize>>);
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

struct Device(Cell<usize>);
impl LibdrmNativePrimaryPlaneResourceDevice for Device {
    fn create_mode_blob_for_selection(
        &self,
        _: LibdrmNativePrimaryPlaneSelection,
    ) -> io::Result<u64> {
        unreachable!()
    }
    fn create_mode_blob(&self, _: drm::control::Mode) -> io::Result<u64> {
        unreachable!()
    }
    fn add_scanout_framebuffer_with_modifiers<B: drm::buffer::PlanarBuffer + ?Sized>(
        &self,
        _: &B,
    ) -> io::Result<drm::control::framebuffer::Handle> {
        unreachable!()
    }
    fn add_scanout_framebuffer_without_modifiers<B: drm::buffer::PlanarBuffer + ?Sized>(
        &self,
        _: &B,
    ) -> io::Result<drm::control::framebuffer::Handle> {
        unreachable!()
    }
    fn add_legacy_scanout_framebuffer<B: drm::buffer::Buffer + ?Sized>(
        &self,
        _: &B,
        _: u32,
        _: u32,
    ) -> io::Result<drm::control::framebuffer::Handle> {
        unreachable!()
    }
    fn destroy_scanout_framebuffer(&self, _: drm::control::framebuffer::Handle) -> io::Result<()> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
    fn destroy_mode_blob(&self, _: u64) -> io::Result<()> {
        unreachable!()
    }
}

fn submitted(
    native: Option<crate::LiveNativeFrameIdentity>,
    drops: &Rc<Cell<usize>>,
) -> LiveRenderedOutputState {
    let output = HeadlessOutput::deterministic();
    let mut state = LiveRenderedOutputState::ready(output);
    state.retain_rendered_primary_plane_displayed_submission = true;
    let submission = LiveRenderedPrimaryPlaneScanoutSubmission {
        scanout_buffer: Box::new(Owner(drops.clone())) as Box<dyn Any>,
        correlation: Some(crate::LiveRendererFrameCorrelation {
            native,
            request: None,
            trace: None,
            direct_scanout: None,
        }),
        primary_plane: LibdrmNativePrimaryPlaneScanoutSubmission {
            resources: LibdrmNativePrimaryPlaneResourceBundle::new(
                NonZeroU32::new(10).unwrap().into(),
                None,
                output.size,
            ),
            completion_fence: None,
        },
        submitted_after_page_flip_serial: Some(3),
        layout_witness: None,
    };
    state.scanout_custody.accept_submission(submission).unwrap();
    state
}

fn callback() -> LivePageFlipCallbackReport {
    LivePageFlipCallbackReport {
        decision: LivePageFlipCallbackDecision::Accepted,
        event: LivePageFlipEvent {
            status: LivePageFlipEventStatus::Presented,
            frame_serial: Some(4),
        },
    }
}

#[test]
fn independent_native_witness_refuses_wrong_scope_without_moving_submission() {
    for persistent in [false, true] {
        native_witness_matrix(persistent);
    }
}

fn native_witness_matrix(persistent: bool) {
    let owner = crate::NativeFrameOwner::new();
    let output = OutputId::from_raw(1);
    let head = sophia_engine::RenderHeadId::from_raw(2);
    let exact = owner.frame(output, head, 3, 4);
    let drops = Rc::new(Cell::new(0));
    let device = Device(Cell::new(0));
    let mut state = submitted(Some(exact), &drops);
    state.retain_rendered_primary_plane_displayed_submission = persistent;
    for authority in [
        RenderedRetirementAuthority::Legacy,
        RenderedRetirementAuthority::Native(None),
        RenderedRetirementAuthority::Native(Some(
            crate::NativeFrameOwner::new().frame(output, head, 3, 4),
        )),
        RenderedRetirementAuthority::Native(Some(owner.frame(OutputId::from_raw(9), head, 3, 4))),
        RenderedRetirementAuthority::Native(Some(owner.frame(
            output,
            sophia_engine::RenderHeadId::from_raw(9),
            3,
            4,
        ))),
        RenderedRetirementAuthority::Native(Some(owner.frame(output, head, 9, 4))),
        RenderedRetirementAuthority::Native(Some(owner.frame(output, head, 3, 9))),
    ] {
        state.retirement_authority = authority;
        let report = retire_tracked_output_after_page_flip(&mut state, &device, &callback());
        assert_eq!(
            report.status,
            LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::WaitingForAcceptedPageFlip
        );
        assert_eq!(report.runtime_scanout_state, None);
        assert!(state.pending_runtime_scanout_states.is_empty());
        assert_eq!(
            state
                .scanout_custody
                .submitted()
                .unwrap()
                .correlation()
                .unwrap()
                .native,
            Some(exact)
        );
        assert!(state.scanout_custody.displayed().is_none());
        assert_eq!(drops.get(), 0);
        assert_eq!(device.0.get(), 0);
    }
    state.retirement_authority = RenderedRetirementAuthority::Native(Some(exact));
    let report = retire_tracked_output_after_page_flip(&mut state, &device, &callback());
    assert_eq!(
        report.status,
        LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip
    );
    assert_eq!(
        report.runtime_scanout_state,
        Some(RuntimeScanoutState::Retired)
    );
    assert!(!state.in_flight());
    assert_eq!(drops.get(), usize::from(!persistent));
    assert_eq!(device.0.get(), usize::from(!persistent));
    let duplicate = retire_tracked_output_after_page_flip(&mut state, &device, &callback());
    assert_eq!(
        duplicate.status,
        LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::NoSubmission
    );
    assert_eq!(state.pending_runtime_scanout_states.len(), 1);
    let teardown = state.scanout_custody.retire_displayed(&device).unwrap();
    assert_eq!(teardown.is_some(), persistent);
    assert!(teardown.is_none_or(|result| result.released));
    assert_eq!(drops.get(), 1);
    assert_eq!(device.0.get(), 1);
}

#[test]
fn native_absence_never_authorizes_an_unidentified_legacy_payload() {
    for persistent in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let device = Device(Cell::new(0));
        let mut state = submitted(None, &drops);
        state.retain_rendered_primary_plane_displayed_submission = persistent;
        state.retirement_authority = RenderedRetirementAuthority::Native(None);
        let refused = retire_tracked_output_after_page_flip(&mut state, &device, &callback());
        assert_eq!(refused.runtime_scanout_state, None);
        assert!(state.in_flight());
        assert!(state.scanout_custody.displayed().is_none());
        assert!(state.pending_runtime_scanout_states.is_empty());
        assert_eq!((drops.get(), device.0.get()), (0, 0));
        state.retirement_authority = RenderedRetirementAuthority::Legacy;
        let presented = retire_tracked_output_after_page_flip(&mut state, &device, &callback());
        assert_eq!(
            presented.runtime_scanout_state,
            Some(RuntimeScanoutState::Retired)
        );
        assert_eq!(drops.get(), usize::from(!persistent));
        state.scanout_custody.retire_displayed(&device).unwrap();
        assert_eq!((drops.get(), device.0.get()), (1, 1));
    }
}

#[test]
fn missing_presentation_authority_does_not_prevent_head_loss_cancellation() {
    for persistent in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let device = Device(Cell::new(0));
        let mut state = submitted(None, &drops);
        state.retain_rendered_primary_plane_displayed_submission = persistent;
        state.retirement_authority = RenderedRetirementAuthority::Native(None);
        state.lost_heads.insert(7);
        let report = retire_tracked_output_after_page_flip(&mut state, &device, &callback());
        assert_eq!(
            report.status,
            LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::HeadLost
        );
        assert_eq!(
            report.runtime_scanout_state,
            Some(RuntimeScanoutState::Rejected)
        );
        assert!(!state.in_flight());
        assert!(state.scanout_custody.displayed().is_none());
        assert_eq!((drops.get(), device.0.get()), (1, 1));
    }
}
