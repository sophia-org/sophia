//! Capture follows native custody, never a render request or a scene counter.
use super::*;

/// Trusted Session facts. Equality covers the full output incarnation, not
/// just its numeric output ID or a scene generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveOutputCaptureTarget {
    pub owner: u64,
    pub output: OutputId,
    pub primary: sophia_engine::RenderHeadId,
    pub heads: Vec<sophia_engine::HeadRenderTarget>,
}

impl LiveProductionNativeScanout {
    pub fn output_capture_validity(
        &mut self,
        output: OutputId,
    ) -> Option<LiveOutputCaptureValidity> {
        self.output_capture_target(output)?;
        Some(self.capture_validity.entry(output).or_default().0.clone())
    }
    pub fn output_capture_target(&self, output: OutputId) -> Option<LiveOutputCaptureTarget> {
        if !self.output_topology_allows_frame_service() || self.has_preview_frame_failure(output) {
            return None;
        }
        let primary = self.heads.get(self.primary_head_index(output)?)?.head;
        let heads = self.head_render_targets(output);
        if heads.is_empty() || heads.len() != self.head_indices(output).len() {
            return None;
        }
        Some(LiveOutputCaptureTarget {
            owner: self.native_frame_owner.raw(),
            output,
            primary,
            heads,
        })
    }

    /// Names come from trusted policy. An unqualified name must resolve to one
    /// physical head even when more than one GPU is present.
    pub fn output_capture_allowed(&self, output: OutputId, allowed: &[String]) -> bool {
        let Some(primary) = self
            .primary_head_index(output)
            .map(|index| self.heads[index].head)
        else {
            return false;
        };
        allowed.iter().any(|name| {
            let mut matches = self.head_table.records().iter().filter(|record| {
                record.connector_name == *name
                    || (!name.contains('/')
                        && record.connector_name.rsplit('/').next() == Some(name.as_str()))
            });
            matches.next().is_some_and(|record| record.head == primary) && matches.next().is_none()
        })
    }
    /// Schedule readback for the exact queued composition on every enabled
    /// head. Session must authorize the recipient before requesting this work.
    /// No later frame inherits a request that was superseded.
    pub fn request_pending_output_capture(
        &mut self,
        output: OutputId,
        frame: LiveProductionNativeFrameId,
    ) -> bool {
        let indices = self.head_indices(output);
        if indices.is_empty()
            || indices
                .iter()
                .any(|index| !self.exporters[*index].worker_enabled())
        {
            return false;
        }
        // Admission owns the complete lowered batch here. The exporter does
        // not receive it until activation; inspecting its pending slot now
        // would miss every newly admitted capture.
        self.deferred_mirror_generations
            .request_capture(output, frame)
    }

    pub fn output_capture_frame_retained(&self, output: OutputId, frame: u64) -> bool {
        self.deferred_mirror_generations
            .get(output)
            .is_some_and(|generation| generation.frame.raw() == frame)
            || self.head_indices(output).into_iter().any(|index| {
                let head = &self.heads[index];
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
    }

    /// Return the primary head's immutable image only when every enabled head
    /// displays the same native frame. This is presentation evidence, not a
    /// recipient grant. Output replacement must still revoke existing grants.
    pub fn presented_output_capture(
        &self,
        output: OutputId,
        runtimes: &crate::LiveProductionOutputRuntimeSet,
    ) -> Option<crate::LiveNativeFrameCapture> {
        let indices = self.head_indices(output);
        let primary = self.primary_head_index(output)?;
        let content = self.heads[primary].presented_content?;
        if self.mirror_generation_failed(output) {
            return None;
        }
        let mut result = None;
        for index in &indices {
            let head = &self.heads[*index];
            if head.presented_content?.frame() != content.frame() {
                return None;
            }
            let expected = self.native_frame_identity(*index, output, content.frame());
            let capture = if indices.len() == 1 {
                runtimes.capture_for(expected)?
            } else {
                let group = self.output_lifecycles.get(&output)?;
                if !group.converged()
                    || group.failed()
                    || group.output() != output
                    || group.heads().count() != indices.len()
                    || group.displayed_frame(head.head) != Some(content.frame())
                {
                    return None;
                }
                head.scanout_custody.capture_for(expected)?
            };
            if *index == primary {
                result = Some(capture);
            }
        }
        result
    }
}
