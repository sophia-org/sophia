//! Multi-head target facts and fake worker/device completions. Admission,
//! installation, real Pending Mixed owners, custody and completion are shared
//! with production. This does not instantiate a card or native exporter.
use super::*;
use crate::{
    CompositionInstallation, CompositionInstaller, LiveProductionMirrorGroupLifecycle,
    LiveProductionQueuedMirrorHeadFrame, NativeCompositionInstallationHead,
};

struct Head {
    target: HeadRenderTarget,
    pending: Option<crate::PendingRenderedFrame>,
    prepared: Option<crate::BoxedRenderedPrimaryPlaneScanoutSubmission>,
    content: Option<crate::LiveProductionScanoutContent>,
    presented: Option<crate::LiveProductionScanoutContent>,
    custody: crate::PersistentScanoutCustody,
    frames: OutputFramePresentationState,
    last_serial: u64,
    history: crate::LiveRendererSlotDamageHistory,
}

struct MirroredTarget {
    owner: crate::NativeFrameOwner,
    outputs: BTreeMap<OutputId, Vec<usize>>,
    heads: Vec<Head>,
    queue: crate::DeferredNativeCompositions,
    next: u64,
    groups: BTreeMap<OutputId, LiveProductionMirrorGroupLifecycle>,
    cohorts: BTreeMap<(OutputId, u64), sophia_engine::OutputPresentationCohort>,
    device: Device,
    owners: Rc<Cell<usize>>,
    wrong_target: Option<usize>,
    refuse_reservation: bool,
    installed_heads: usize,
    serial: u64,
}

impl MirroredTarget {
    fn new(outputs: &[HeadlessOutput]) -> Self {
        let mut heads = Vec::new();
        let mut indices = BTreeMap::new();
        let mut groups = BTreeMap::new();
        for output in outputs {
            let start = heads.len();
            for index in start..start + 2 {
                heads.push(Head {
                    target: HeadRenderTarget {
                        output: output.id,
                        head: RenderHeadId::from_raw(index as u64 + 1),
                        target_generation: 1,
                        native_size: output.size,
                        scale: 1,
                        refresh_millihz: 60_000,
                        transform: OutputTransform::Normal,
                        mapping: OutputHeadMapping::Fit,
                    },
                    pending: None,
                    prepared: None,
                    content: None,
                    presented: None,
                    custody: Default::default(),
                    frames: OutputFramePresentationState::new(*output).unwrap(),
                    last_serial: 0,
                    history: crate::LiveRendererSlotDamageHistory::new(),
                });
            }
            indices.insert(output.id, vec![start, start + 1]);
            groups.insert(
                output.id,
                LiveProductionMirrorGroupLifecycle::new(
                    output.id,
                    [heads[start].target.head, heads[start + 1].target.head],
                )
                .unwrap(),
            );
        }
        for group in groups.values_mut() {
            for head in group.heads().collect::<Vec<_>>() {
                group.mark_initialized(head);
            }
        }
        Self {
            owner: crate::NativeFrameOwner::new(),
            outputs: indices,
            heads,
            queue: Default::default(),
            next: 1,
            groups,
            cohorts: BTreeMap::new(),
            device: Device {
                destroyed: Cell::new(0),
                failing: Cell::new(None),
            },
            owners: Rc::new(Cell::new(0)),
            wrong_target: None,
            refuse_reservation: false,
            installed_heads: 0,
            serial: 0,
        }
    }

    fn settled_checksum(&self, output: OutputId) -> Option<u64> {
        let indices = &self.outputs[&output];
        crate::settled_mirror_checksum(
            self.owner,
            output,
            indices.len(),
            self.groups.get(&output),
            indices.iter().map(|index| {
                let head = &self.heads[*index];
                crate::SettledMirrorHead {
                    head: head.target.head,
                    target_generation: head.target.target_generation,
                    idle: !self.queue.pending(output)
                        && head.pending.is_none()
                        && head.prepared.is_none()
                        && head.custody.submitted().is_none()
                        && !head.custody.cleanup_pending(),
                    presented: head.presented,
                    displayed: head
                        .custody
                        .displayed()
                        .and_then(|value| value.correlation())
                        .and_then(|value| value.native),
                }
            }),
        )
    }

    fn protected(&self, output: OutputId) -> bool {
        self.queue.protected(output)
            || self.outputs[&output].iter().any(|index| {
                let head = &self.heads[*index];
                (head.pending.is_some()
                    || head.prepared.is_some()
                    || head.custody.submitted().is_some())
                    && head
                        .content
                        .is_some_and(|content| content.requires_retirement())
            })
    }

    fn ready(&self, output: OutputId) -> bool {
        !self.protected(output)
            && self.outputs[&output].iter().all(|index| {
                let head = &self.heads[*index];
                head.pending.is_none() && head.prepared.is_none() && head.custody.can_submit()
            })
    }

    fn install(&mut self, output: OutputId) -> Result<(), &'static str> {
        let generation = self.queue.take_ready(output, None, None, None).unwrap();
        match crate::install_composition_generation(self, generation) {
            Ok(()) => Ok(()),
            Err((reason, generation)) => {
                assert!(self.queue.retain_refused(generation).is_ok());
                Err(reason)
            }
        }
    }

    fn prepare(&mut self, output: OutputId) {
        for index in &self.outputs[&output] {
            let head = &mut self.heads[*index];
            let frame = head.content.unwrap().frame().raw();
            let pending = head.pending.take().unwrap();
            let submission = copy_submission(
                pending,
                head.target,
                head.last_serial,
                frame * 16 + *index as u64 + 1,
                &self.owners,
            );
            let native = submission.correlation().unwrap().native.unwrap();
            let candidate = sophia_engine::HeadFrameCandidate {
                candidate: sophia_engine::HeadFrameCandidateId::from_raw(
                    frame * 16 + *index as u64 + 1,
                ),
                output,
                scene_generation: frame,
                head: head.target.head,
                target_generation: native.target_generation(),
                logical_content_checksum: head.content.unwrap().logical_checksum().unwrap(),
            };
            assert!(matches!(
                self.cohorts
                    .get_mut(&(output, frame))
                    .unwrap()
                    .mark_prepared(candidate),
                sophia_engine::OutputPresentationTransition::Accepted
                    | sophia_engine::OutputPresentationTransition::PhaseReady
            ));
            assert!(head.prepared.replace(submission).is_none());
        }
        for index in &self.outputs[&output] {
            let head = &mut self.heads[*index];
            let frame = head.content.unwrap().frame();
            let cohort = self.cohorts.get_mut(&(output, frame.raw())).unwrap();
            assert!(cohort.all_prepared());
            head.custody
                .accept_submission(head.prepared.take().unwrap())
                .unwrap();
            assert!(matches!(
                cohort.mark_submitted(head.target.head),
                sophia_engine::OutputPresentationTransition::Accepted
                    | sophia_engine::OutputPresentationTransition::PhaseReady
            ));
            assert!(matches!(
                self.groups
                    .get_mut(&output)
                    .unwrap()
                    .mark_submitted(head.target.head, frame),
                crate::LiveProductionMirrorHeadTransition::Accepted
                    | crate::LiveProductionMirrorHeadTransition::GroupReady
            ));
            head.frames.promote_rendering_to_submitted().unwrap();
        }
    }

    fn flip(&mut self, output: OutputId, sibling: usize) {
        let index = self.outputs[&output][sibling];
        let head = &mut self.heads[index];
        let frame = head.content.unwrap().frame();
        self.serial += 1;
        let result = crate::complete_mirror_head(
            &self.device,
            &mut head.custody,
            self.groups.get_mut(&output).unwrap(),
            self.cohorts.get_mut(&(output, frame.raw())),
            crate::MirrorCompletionWitness {
                expected: self.owner.frame(
                    output,
                    head.target.head,
                    head.target.target_generation,
                    frame.raw(),
                ),
                callback: crate::LivePageFlipCallback {
                    output,
                    head: head.target.head,
                    frame_serial: self.serial,
                },
                last_callback_serial: Some(head.last_serial),
                ust_usec: self.serial * 1000,
            },
        );
        let crate::PersistentFlipOutcome::Presented {
            previous_cleanup, ..
        } = result.physical
        else {
            panic!("physical refusal: {:?}", result.physical)
        };
        if let Some(cleanup) = previous_cleanup {
            assert!(cleanup.released);
            let identity = cleanup.correlation.unwrap().native.unwrap();
            self.cohorts
                .get_mut(&(output, identity.frame()))
                .unwrap()
                .mark_cleanup_complete(identity.head());
        }
        assert!(result.logical.is_some());
        head.last_serial = self.serial;
        head.presented = head.content;
        let presented = head.frames.mark_presented().unwrap();
        head.history.record(
            crate::LiveRendererFrameSlotId::from_index((frame.raw() % 2) as usize).unwrap(),
            presented.snapshot,
        );
        self.cohorts
            .retain(|_, cohort| !cohort.generation_releasable());
    }

    fn drain(&mut self) {
        for output in self.outputs.keys().copied().collect::<Vec<_>>() {
            if self.queue.pending(output) {
                self.install(output).unwrap();
                self.prepare(output);
                self.flip(output, 1);
                self.flip(output, 0);
            }
        }
    }

    fn teardown(&mut self) {
        self.queue.clear();
        for head in &mut self.heads {
            head.pending = None;
            assert!(head.prepared.is_none());
            if head.custody.submitted().is_some() {
                assert!(
                    head.custody
                        .discard_submitted(&self.device)
                        .unwrap()
                        .released
                );
            }
            if head.custody.displayed().is_some() {
                assert!(
                    head.custody
                        .retire_displayed(&self.device)
                        .unwrap()
                        .unwrap()
                        .released
                );
            }
            assert!(!head.custody.cleanup_pending());
        }
        assert_eq!(self.owners.get(), 0);
    }
}

impl CompositionInstaller for MirroredTarget {
    fn current_heads(
        &self,
        installation: CompositionInstallation,
    ) -> Vec<NativeCompositionInstallationHead> {
        self.outputs[&installation.output]
            .iter()
            .map(|index| {
                let head = &self.heads[*index];
                NativeCompositionInstallationHead {
                    index: *index,
                    identity: self.owner.frame(
                        installation.output,
                        head.target.head,
                        head.target.target_generation
                            + u64::from(self.wrong_target == Some(*index)),
                        installation.frame.raw(),
                    ),
                    prepared_cleanup_available: head.prepared.is_none(),
                    protected_frames: [
                        head.content.filter(|_| head.pending.is_some()),
                        None,
                        head.content.filter(|_| head.custody.submitted().is_some()),
                    ]
                    .map(|content| {
                        content
                            .filter(|content| content.requires_retirement())
                            .map(|content| content.frame())
                    }),
                }
            })
            .collect()
    }
    fn reserve(
        &mut self,
        installation: CompositionInstallation,
        current: &[NativeCompositionInstallationHead],
    ) -> Result<(), &'static str> {
        let lifecycle = if self.refuse_reservation {
            None
        } else {
            self.groups.get_mut(&installation.output)
        };
        if let Some(cohort) =
            crate::reserve_composition_lifecycle(installation, current, lifecycle)?
        {
            self.cohorts
                .insert((installation.output, installation.frame.raw()), cohort);
        }
        Ok(())
    }
    fn install_head(
        &mut self,
        installation: CompositionInstallation,
        queued: LiveProductionQueuedMirrorHeadFrame,
    ) {
        assert!(installation.mirrored);
        assert!(matches!(
            queued.frame.direct_scanout,
            sophia_engine::DirectScanoutVerdict::CompositionRequired("mirror_cohort")
        ));
        let head = &mut self.heads[queued.head_index];
        assert!(head.pending.is_none() && head.prepared.is_none());
        head.frames
            .queue(queued.output_damage_snapshot.unwrap())
            .unwrap();
        head.frames.mark_rendering().unwrap();
        head.content = Some(queued.content);
        head.pending = Some(crate::PendingRenderedFrame::Mixed(
            queued.frame,
            Some(queued.identity),
        ));
        self.installed_heads += 1;
    }
}

impl NativeCompositionTarget for MirroredTarget {
    fn frame_owner(&self) -> crate::NativeFrameOwner {
        self.owner
    }
    fn frame_service_available(&self) -> bool {
        true
    }
    fn head_targets(&self, output: OutputId) -> Vec<HeadRenderTarget> {
        self.outputs[&output]
            .iter()
            .map(|index| self.heads[*index].target)
            .collect()
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
        self.queue_scene_batch(
            frames,
            required,
            crate::LiveProductionHeadCompositionContent::Retained,
        )
    }
    fn queue_ordinary_batch(
        &mut self,
        frames: Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>,
    ) -> Result<BTreeMap<OutputId, crate::LiveProductionNativeFrameId>, Box<dyn std::error::Error>>
    {
        self.queue_scene_batch(
            frames,
            &BTreeSet::new(),
            crate::LiveProductionHeadCompositionContent::OrdinaryScene,
        )
    }
    fn retained_repaint_deferred(&self) -> bool {
        self.outputs.keys().any(|output| self.protected(*output))
    }
    fn presented_frame(&self, output: OutputId) -> Option<&OutputFrameDamageSnapshot> {
        self.heads[self.outputs[&output][0]].frames.presented()
    }
}

#[path = "mirrored_intake_tests.rs"]
mod tests;

impl IntegrationTarget for MirroredTarget {
    fn queued(&self) -> &crate::DeferredNativeCompositions {
        &self.queue
    }
    fn drain(&mut self) {
        MirroredTarget::drain(self);
    }
    fn teardown(&mut self) {
        MirroredTarget::teardown(self);
    }
    fn backing_count(&self) -> usize {
        self.owners.get()
    }
}

impl MirroredTarget {
    fn queue_scene_batch(
        &mut self,
        frames: Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>,
        required: &BTreeSet<OutputId>,
        content: crate::LiveProductionHeadCompositionContent,
    ) -> Result<BTreeMap<OutputId, crate::LiveProductionNativeFrameId>, Box<dyn std::error::Error>>
    {
        let states = self
            .outputs
            .iter()
            .map(|(output, indices)| {
                let primary = &self.heads[indices[0]];
                (
                    *output,
                    crate::NativeCompositionOutput {
                        targets: indices
                            .iter()
                            .map(|index| (*index, self.heads[*index].target))
                            .collect(),
                        ready: self.ready(*output),
                        protected: self.protected(*output),
                        available: true,
                        newest: [None, None, None, primary.presented],
                        settled_mirror_checksum: self.settled_checksum(*output),
                    },
                )
            })
            .collect();
        let generations = crate::prepare_native_composition_batch(
            frames,
            required,
            &states,
            self.owner,
            &mut self.next,
            content,
        )
        .map_err(|(reason, _owners)| reason)?;
        self.queue
            .admit_batch(generations, &self.outputs.keys().copied().collect())
            .map_err(|(reason, _owners)| reason.into())
    }
}
