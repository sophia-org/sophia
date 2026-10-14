//! Capture follows native custody, never a render request or a scene counter.
use super::*;

impl LiveProductionNativeScanout {
    /// Schedule readback for the exact queued composition on every enabled
    /// head. Session must authorize the recipient before requesting this work.
    /// No later frame inherits a request that was superseded.
    pub fn request_pending_output_capture(&mut self, output: OutputId) -> bool {
        let indices = self.head_indices(output);
        if indices.is_empty() {
            return false;
        }
        let mut expected = Vec::with_capacity(indices.len());
        let mut frame = None;
        for index in indices {
            let Some(identity) = self.exporters[index]
                .queued_frame_correlation()
                .and_then(|c| c.native)
            else {
                return false;
            };
            let head = &self.heads[index];
            if !self.exporters[index].can_capture_pending_frame(identity)
                || identity.owner() != self.native_frame_owner.raw()
                || identity.output() != output
                || identity.head() != head.head
                || identity.target_generation() != head.target_generation
                || frame.is_some_and(|frame| frame != identity.frame())
            {
                return false;
            }
            frame = Some(identity.frame());
            expected.push((index, identity));
        }
        expected
            .into_iter()
            .all(|(index, identity)| self.exporters[index].request_pending_frame_capture(identity))
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
