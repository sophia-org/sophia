//! Borrowed installation checks before a queued generation transfers any owner.
use super::composition_queue::{
    LiveProductionQueuedMirrorGeneration, LiveProductionQueuedMirrorHeadFrame,
};
use super::*;

/// Current facts captured by the native owner, independently of the queued payload.
/// This is a call-local validation view, not another ownership ledger.
pub(crate) struct NativeCompositionInstallationHead {
    pub index: usize,
    pub identity: crate::LiveNativeFrameIdentity,
    pub prepared_cleanup_available: bool,
    pub protected_frames: [Option<LiveProductionNativeFrameId>; 3],
}

pub(crate) fn validate_composition_installation(
    generation: &LiveProductionQueuedMirrorGeneration,
    current: &[NativeCompositionInstallationHead],
) -> Result<(), &'static str> {
    if generation.heads.is_empty()
        || generation
            .heads
            .iter()
            .any(|head| head.content.frame() != generation.frame)
    {
        return Err("mirror generation has invalid or mismatched frame identity");
    }
    if current.len() != generation.heads.len()
        || current
            .iter()
            .zip(&generation.heads)
            .any(|(current, queued)| current.index != queued.head_index)
    {
        return Err("mirror generation does not cover every physical head exactly once");
    }
    for (current, queued) in current.iter().zip(&generation.heads) {
        if current.identity != queued.identity {
            return Err("mirror generation does not name the current native targets");
        }
        if !current.prepared_cleanup_available {
            return Err("mirror generation waits for prepared-owner cleanup capacity");
        }
        if current
            .protected_frames
            .iter()
            .flatten()
            .any(|old| *old != generation.frame)
        {
            return Err("composition installation waits for an existing distinct retirement");
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) struct CompositionInstallation {
    pub output: OutputId,
    pub frame: LiveProductionNativeFrameId,
    pub checksum: Option<u64>,
    pub mirrored: bool,
}

/// The final adapter cannot refuse after receiving a head. All fallible checks
/// and cohort reservation happen while the complete generation remains owned
/// by the shared installer. This promises returned-refusal retention, not unwind recovery.
pub(crate) trait CompositionInstaller {
    fn current_heads(
        &self,
        installation: CompositionInstallation,
    ) -> Vec<NativeCompositionInstallationHead>;
    fn reserve(
        &mut self,
        installation: CompositionInstallation,
        current: &[NativeCompositionInstallationHead],
    ) -> Result<(), &'static str>;
    fn install_head(
        &mut self,
        installation: CompositionInstallation,
        head: LiveProductionQueuedMirrorHeadFrame,
    );
}

pub(crate) fn install_composition_generation<I: CompositionInstaller>(
    installer: &mut I,
    generation: LiveProductionQueuedMirrorGeneration,
) -> Result<(), (&'static str, LiveProductionQueuedMirrorGeneration)> {
    let installation = CompositionInstallation {
        output: generation.output,
        frame: generation.frame,
        checksum: generation.logical_checksum(),
        mirrored: generation.heads.len() > 1,
    };
    let current = installer.current_heads(installation);
    if let Err(reason) = validate_composition_installation(&generation, &current) {
        return Err((reason, generation));
    }
    if let Err(reason) = installer.reserve(installation, &current) {
        return Err((reason, generation));
    }
    for mut head in generation.heads {
        if installation.mirrored {
            head.frame.direct_scanout =
                sophia_engine::DirectScanoutVerdict::CompositionRequired("mirror_cohort");
        }
        installer.install_head(installation, head);
    }
    Ok(())
}

impl CompositionInstaller for LiveProductionNativeScanout {
    fn current_heads(
        &self,
        installation: CompositionInstallation,
    ) -> Vec<NativeCompositionInstallationHead> {
        self.head_indices(installation.output)
            .into_iter()
            .map(|index| {
                let head = &self.heads[index];
                NativeCompositionInstallationHead {
                    index,
                    identity: self.native_frame_identity(
                        index,
                        installation.output,
                        installation.frame,
                    ),
                    prepared_cleanup_available: head.prepared_scanout.is_none()
                        || head.scanout_custody.can_cancel_prepared(),
                    protected_frames: [
                        head.pending_content,
                        head.rendering_content,
                        head.submitted_content,
                    ]
                    .map(|content| {
                        content
                            .filter(|value| value.requires_retirement())
                            .map(|value| value.frame())
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
        if let Some(cohort) = reserve_composition_lifecycle(
            installation,
            current,
            self.output_lifecycles.get_mut(&installation.output),
        )? {
            self.output_cohorts
                .insert((installation.output, installation.frame), cohort);
        }
        Ok(())
    }

    fn install_head(
        &mut self,
        installation: CompositionInstallation,
        queued: LiveProductionQueuedMirrorHeadFrame,
    ) {
        if let Some(old_frame) = self.heads[queued.head_index]
            .prepared_group_frame
            .filter(|old| *old != installation.frame)
            && let Some(prepared) = self.heads[queued.head_index].prepared_scanout.take()
        {
            assert!(
                self.cancel_prepared_head_owner(queued.head_index, prepared),
                "cleanup capacity checked before generation transfer"
            );
            if let Some(cohort) = self
                .output_cohorts
                .get_mut(&(installation.output, old_frame))
            {
                let _ = cohort.mark_skipped(self.heads[queued.head_index].head);
            }
        }
        if let Some(old_frame) = self.heads[queued.head_index]
            .pending_content
            .map(LiveProductionScanoutContent::frame)
            .filter(|old| *old != installation.frame)
            && let Some(cohort) = self
                .output_cohorts
                .get_mut(&(installation.output, old_frame))
        {
            let _ = cohort.mark_skipped(self.heads[queued.head_index].head);
        }
        let identity = queued.identity;
        let (head, exporter) = self.head_and_exporter(queued.head_index, installation.output);
        if let Some(checksum) = installation.checksum {
            head.last_checksum = checksum;
            head.pending_nonzero_pixel_bytes = queued.cpu_nonzero_pixel_bytes;
        }
        head.pending_content = Some(queued.content);
        head.queue_output_damage_snapshot(queued.output_damage_snapshot);
        exporter.set_pending_identified_mixed_frame(queued.frame, Some(identity));
    }
}

/// Eligibility, construction and the only fallible reservation transition.
/// begin() refuses before mutation; no cohort is returned or published on Err.
pub(crate) fn reserve_composition_lifecycle(
    installation: CompositionInstallation,
    current: &[NativeCompositionInstallationHead],
    lifecycle: Option<&mut LiveProductionMirrorGroupLifecycle>,
) -> Result<Option<sophia_engine::OutputPresentationCohort>, &'static str> {
    if !installation.mirrored {
        return Ok(None);
    }
    let lifecycle = lifecycle.ok_or("mirror generation targets an unregistered output")?;
    let heads = current
        .iter()
        .map(|head| head.identity.head())
        .collect::<BTreeSet<_>>();
    if lifecycle.output() != installation.output || !lifecycle.heads().eq(heads.iter().copied()) {
        return Err("mirror lifecycle does not name the current physical heads");
    }
    if !lifecycle.initialized() {
        return Ok(None);
    }
    let cohort = sophia_engine::OutputPresentationCohort::new(
        installation.output,
        installation.frame.raw(),
        lifecycle.primary_head(),
        heads,
    )
    .ok_or("mirror generation could not create its presentation cohort")?;
    if lifecycle.begin(installation.frame) != LiveProductionMirrorGroupBegin::Started {
        return Err("mirror generation could not reserve its lifecycle");
    }
    Ok(Some(cohort))
}
