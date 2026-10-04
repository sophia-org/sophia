//! Only an unrelated, exactly unchanged, retired singleton can stay idle.
use super::*;

impl LiveProductionVisualRuntime {
    pub(in crate::production_visual_runtime) fn software_present_output_is_idle<
        T: NativeCompositionTarget,
    >(
        &self,
        native: &T,
        output: OutputId,
        frames: &[crate::LiveProductionHeadCompositionFrame],
        submissions: &[LiveProductionSoftwarePresentSubmission],
    ) -> bool {
        if self.session_lock.is_some()
            || !self.outputs.native_initialized(output)
            || self.ordinary_repaints_pending.contains(&output)
            || self
                .retained_projection_retirements
                .keys()
                .any(|(owed, _)| *owed == output)
            || self
                .queued_shell_retirements
                .keys()
                .any(|(owed, _)| *owed == output)
        {
            return false;
        }
        let [frame] = frames else {
            return false;
        };
        let Some(presented) = native.idle_presented_frame(output, &self.outputs) else {
            return false;
        };
        // Legacy CPU frames may retire without a damage snapshot, leaving
        // output_frames.presented() from an older frame. Head compositions
        // require a snapshot at admission; only those can prove reuse here.
        match presented.content {
            crate::LiveProductionScanoutContent::Cpu { .. } => return false,
            crate::LiveProductionScanoutContent::HeadComposition { .. }
            | crate::LiveProductionScanoutContent::MixedPresent { .. }
            | crate::LiveProductionScanoutContent::RetainedMixed { .. } => {}
        }
        let identity = presented.identity;
        let before = presented.snapshot;
        // A snapshot on an old target or from another native lifetime is not
        // proof about what the current head is scanning out.
        if identity.frame() != presented.content.frame().raw()
            || identity.owner() != native.frame_owner().raw()
            || identity.output() != output
            || identity.head() != frame.head
            || identity.target_generation() != frame.target_generation
        {
            return false;
        }
        let Some(after) = frame.frame.output_damage_snapshot.as_ref() else {
            return false;
        };
        let samples_present = |snapshot: &OutputFrameDamageSnapshot| {
            submissions.iter().any(|submission| {
                snapshot
                    .surfaces
                    .iter()
                    .any(|surface| surface.surface == submission.surface)
                    || snapshot
                        .compositor_display_list
                        .surface_instances()
                        .any(|instance| instance.source == submission.surface)
            })
        };
        // An empty damage region is not permission to discharge a Present,
        // a move off this output, or a preview's actual presentation.
        if samples_present(before) || samples_present(after) {
            return false;
        }
        // Compare semantic frame facts too: publication stamps and logical
        // input geometry must not be discarded just because pixels match.
        // Damage history contains other outputs' surfaces; reduce its sampled
        // identities instead of requiring that global history to stay equal.
        before.output == after.output
            && before.surfaces == after.surfaces
            && before.compositor_display_list == after.compositor_display_list
            && before.software_cursor == after.software_cursor
            && sophia_engine::output_frame_damage(Some(before), after)
                .is_ok_and(|damage| damage.rects.is_empty())
    }
}
