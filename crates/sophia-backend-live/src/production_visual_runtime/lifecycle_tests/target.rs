use super::*;
use std::{any::Any, cell::Cell, num::NonZeroU32, rc::Rc};

pub(super) struct Target {
    owner: crate::NativeFrameOwner,
    outputs: BTreeMap<OutputId, HeadRenderTarget>,
    pub queue: crate::DeferredNativeCompositions,
    pub next: u64,
    pub reject_output: Option<OutputId>,
    frames: BTreeMap<OutputId, OutputFramePresentationState>,
    rendering: BTreeMap<
        OutputId,
        (
            crate::PendingRenderedFrame,
            crate::LiveProductionScanoutContent,
        ),
    >,
    submitted: BTreeMap<OutputId, crate::LiveProductionScanoutContent>,
    newest: BTreeMap<OutputId, crate::LiveProductionScanoutContent>,
    custody: BTreeMap<OutputId, crate::PersistentScanoutCustody>,
    serial: u64,
    device: Device,
    history: crate::LiveRendererSlotDamageHistory,
    pub backing_owners: Rc<Cell<usize>>,
}

impl Target {
    pub fn new(outputs: &[HeadlessOutput]) -> Self {
        Self {
            owner: crate::NativeFrameOwner::new(),
            outputs: outputs
                .iter()
                .map(|output| {
                    (
                        output.id,
                        HeadRenderTarget {
                            output: output.id,
                            head: RenderHeadId::from_raw(output.id.raw()),
                            target_generation: 1,
                            native_size: output.size,
                            scale: 1,
                            refresh_millihz: 60_000,
                            transform: OutputTransform::Normal,
                            mapping: OutputHeadMapping::Fit,
                        },
                    )
                })
                .collect(),
            queue: Default::default(),
            next: 1,
            reject_output: None,
            frames: outputs
                .iter()
                .map(|output| {
                    (
                        output.id,
                        OutputFramePresentationState::new(*output).unwrap(),
                    )
                })
                .collect(),
            rendering: BTreeMap::new(),
            submitted: BTreeMap::new(),
            newest: BTreeMap::new(),
            custody: BTreeMap::new(),
            serial: 0,
            device: Device {
                destroyed: Cell::new(0),
                failing: Cell::new(None),
            },
            history: crate::LiveRendererSlotDamageHistory::new(),
            backing_owners: Rc::new(Cell::new(0)),
        }
    }

    pub fn drain(&mut self) {
        for output in self.outputs.keys().copied().collect::<Vec<_>>() {
            if self.queue.pending(output) {
                self.complete(output);
            }
        }
    }

    pub fn teardown(&mut self) {
        self.queue.clear();
        for custody in self.custody.values_mut() {
            assert!(
                custody
                    .retire_displayed(&self.device)
                    .unwrap()
                    .is_none_or(|result| result.released)
            );
            assert!(!custody.cleanup_pending());
        }
    }

    pub fn fail_cleanup(&self, framebuffer: Option<u32>) {
        self.device.failing.set(framebuffer);
    }

    pub fn retry_cleanup(&mut self, output: OutputId) -> Option<bool> {
        self.custody
            .get_mut(&output)
            .and_then(|custody| custody.retry_cleanup(&self.device))
            .map(|result| result.released)
    }

    pub fn complete(&mut self, output: OutputId) {
        self.begin_render(output);
        self.finish_render(output);
        assert!(self.flip(output, None));
    }

    pub fn begin_render(&mut self, output: OutputId) {
        assert!(!self.rendering.contains_key(&output));
        let queued = self.queue.get(output).unwrap();
        let target = self.outputs[&output];
        let current = [crate::NativeCompositionInstallationHead {
            index: self.outputs.keys().position(|id| *id == output).unwrap(),
            identity: self.owner.frame(
                output,
                target.head,
                target.target_generation,
                queued.frame.raw(),
            ),
            prepared_cleanup_available: true, // this adapter has no prepared owner
            protected_frames: [
                None,
                None,
                self.submitted
                    .get(&output)
                    .filter(|content| content.requires_retirement())
                    .map(|content| content.frame()),
            ],
        }];
        crate::validate_composition_installation(queued, &current).unwrap();
        let generation = self.queue.take_ready(output, None, None, None).unwrap();
        assert_eq!(generation.heads.len(), 1);
        let head = generation.heads.into_iter().next().unwrap();
        let target = self.outputs[&output];
        assert_eq!(
            head.identity,
            self.owner.frame(
                output,
                target.head,
                target.target_generation,
                generation.frame.raw()
            )
        );
        let state = self.frames.get_mut(&output).unwrap();
        state.queue(head.output_damage_snapshot.unwrap()).unwrap();
        state.mark_rendering().unwrap();
        self.rendering.insert(
            output,
            (
                crate::PendingRenderedFrame::Mixed(head.frame, Some(head.identity)),
                head.content,
            ),
        );
    }

    pub fn finish_render(&mut self, output: OutputId) {
        let (pending, content) = self.rendering.remove(&output).unwrap();
        let crate::PendingRenderedFrame::Mixed(frame, native) = pending else {
            unreachable!()
        };
        // Simulated worker completion copies the actual lowered input bytes.
        // The returned backing owns a different allocation, not a source lease.
        let mut copied = Vec::new();
        for layer in &frame.layers {
            if let LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } = layer {
                copied.extend_from_slice(&buffer.bytes);
            }
        }
        drop(frame);
        let target = self.outputs[&output];
        let custody = self.custody.entry(output).or_default();
        self.backing_owners.set(self.backing_owners.get() + 1);
        let submission = crate::LiveRenderedPrimaryPlaneScanoutSubmission {
            scanout_buffer: Box::new(CopiedBacking {
                bytes: copied,
                live: self.backing_owners.clone(),
            }) as Box<dyn Any>,
            correlation: Some(crate::LiveRendererFrameCorrelation {
                native,
                request: None,
                trace: None,
                direct_scanout: None,
            }),
            primary_plane: crate::LibdrmNativePrimaryPlaneScanoutSubmission {
                resources: crate::LibdrmNativePrimaryPlaneResourceBundle::new(
                    NonZeroU32::new(u32::try_from(content.frame().raw()).unwrap())
                        .unwrap()
                        .into(),
                    None,
                    target.native_size,
                ),
                completion_fence: None,
            },
            submitted_after_page_flip_serial: Some(self.serial),
            layout_witness: None,
        };
        custody.accept_submission(submission).unwrap();
        self.submitted.insert(output, content);
        self.frames
            .get_mut(&output)
            .unwrap()
            .promote_rendering_to_submitted()
            .unwrap();
    }

    pub fn flip(
        &mut self,
        output: OutputId,
        wrong: Option<crate::LiveNativeFrameIdentity>,
    ) -> bool {
        let content = self.submitted[&output];
        let target = self.outputs[&output];
        let expected = wrong.unwrap_or_else(|| {
            self.owner.frame(
                output,
                target.head,
                target.target_generation,
                content.frame().raw(),
            )
        });
        self.serial += 1;
        let result = self.custody.get_mut(&output).unwrap().present(
            &self.device,
            &crate::LivePageFlipCallbackReport {
                decision: crate::LivePageFlipCallbackDecision::Accepted,
                event: crate::LivePageFlipEvent {
                    status: crate::LivePageFlipEventStatus::Presented,
                    frame_serial: Some(self.serial),
                },
            },
            Some(expected),
        );
        if !matches!(result, crate::PersistentFlipOutcome::Presented { .. }) {
            return false;
        }
        self.submitted.remove(&output);
        let presented = self
            .frames
            .get_mut(&output)
            .unwrap()
            .mark_presented()
            .unwrap();
        self.history.record(
            crate::LiveRendererFrameSlotId::from_index((output.raw() - 1) as usize).unwrap(),
            presented.snapshot,
        );
        self.newest.insert(output, content);
        true
    }

    fn ready(&self, output: OutputId) -> bool {
        self.outputs.contains_key(&output)
            && !self.protected(output)
            && self
                .custody
                .get(&output)
                .is_none_or(crate::PersistentScanoutCustody::can_submit)
    }

    fn protected(&self, output: OutputId) -> bool {
        self.queue.protected(output)
            || self
                .rendering
                .get(&output)
                .is_some_and(|(_, content)| content.requires_retirement())
            || self
                .submitted
                .get(&output)
                .is_some_and(|content| content.requires_retirement())
    }
}

impl NativeCompositionTarget for Target {
    fn frame_service_available(&self) -> bool {
        true
    }
    fn head_targets(&self, output: OutputId) -> Vec<HeadRenderTarget> {
        self.outputs.get(&output).copied().into_iter().collect()
    }
    fn has_in_flight_direct(&self) -> bool {
        false
    }
    fn required_outputs_ready(&self, outputs: &BTreeSet<OutputId>) -> bool {
        outputs.iter().all(|output| self.ready(*output))
    }
    fn queue_retained_batch(
        &mut self,
        frames: Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>,
        required: &BTreeSet<OutputId>,
    ) -> Result<BTreeMap<OutputId, crate::LiveProductionNativeFrameId>, Box<dyn std::error::Error>>
    {
        let states = self
            .outputs
            .iter()
            .enumerate()
            .map(|(index, (output, target))| {
                (
                    *output,
                    crate::NativeCompositionOutput {
                        targets: vec![(index, *target)],
                        ready: self.ready(*output),
                        protected: self.protected(*output),
                        available: self.reject_output != Some(*output),
                        newest: [None, None, None, self.newest.get(output).copied()],
                    },
                )
            })
            .collect();
        let prepared = crate::prepare_native_composition_batch(
            frames,
            required,
            &states,
            self.owner,
            &mut self.next,
            crate::LiveProductionHeadCompositionContent::Retained,
        )
        .map_err(|(reason, _owners)| reason)?;
        self.queue
            .admit_batch(prepared, &self.outputs.keys().copied().collect())
            .map_err(|(reason, _owners)| reason.into())
    }
    fn retained_repaint_deferred(&self) -> bool {
        self.outputs.keys().any(|output| self.protected(*output))
    }
    fn presented_frame(&self, output: OutputId) -> Option<&OutputFrameDamageSnapshot> {
        self.frames
            .get(&output)
            .and_then(OutputFramePresentationState::presented)
    }
}

use crate::prelude::*;
use std::io;
#[derive(Debug)]
struct CopiedBacking {
    bytes: Vec<u8>,
    live: Rc<Cell<usize>>,
}
impl Drop for CopiedBacking {
    fn drop(&mut self) {
        self.bytes.clear();
        self.live.set(self.live.get() - 1);
    }
}

struct Device {
    destroyed: Cell<usize>,
    failing: Cell<Option<u32>>,
}
impl LibdrmNativePrimaryPlaneResourceDevice for Device {
    fn create_mode_blob_for_selection(
        &self,
        _: LibdrmNativePrimaryPlaneSelection,
    ) -> io::Result<u64> {
        unreachable!()
    }
    fn create_mode_blob(&self, _: ::drm::control::Mode) -> io::Result<u64> {
        unreachable!()
    }
    fn add_scanout_framebuffer_with_modifiers<B: ::drm::buffer::PlanarBuffer + ?Sized>(
        &self,
        _: &B,
    ) -> io::Result<::drm::control::framebuffer::Handle> {
        unreachable!()
    }
    fn add_scanout_framebuffer_without_modifiers<B: ::drm::buffer::PlanarBuffer + ?Sized>(
        &self,
        _: &B,
    ) -> io::Result<::drm::control::framebuffer::Handle> {
        unreachable!()
    }
    fn add_legacy_scanout_framebuffer<B: ::drm::buffer::Buffer + ?Sized>(
        &self,
        _: &B,
        _: u32,
        _: u32,
    ) -> io::Result<::drm::control::framebuffer::Handle> {
        unreachable!()
    }
    fn destroy_scanout_framebuffer(
        &self,
        handle: ::drm::control::framebuffer::Handle,
    ) -> io::Result<()> {
        if self.failing.get() == Some(u32::from(handle)) {
            return Err(io::Error::other("injected framebuffer cleanup failure"));
        }
        self.destroyed.set(self.destroyed.get() + 1);
        Ok(())
    }
    fn destroy_mode_blob(&self, _: u64) -> io::Result<()> {
        unreachable!()
    }
}
