use super::*;

impl LiveProductionNativeHead {
    pub(super) fn install_topology_presentation(&mut self, frames: OutputFramePresentationState) {
        // Apply and rollback reuse the physical head and card event route.
        // Keep its completion authority: kernel event serials and synthetic
        // fence serials have different bases, so a late event cannot be joined
        // to the fence-retired submission after a logical topology change.
        // A new native owner constructs fresh heads instead of using this reset.
        self.pending_callback = None;
        self.completion_fence_status = crate::LibdrmNativeCompletionFenceStatus::Unsupported;
        self.pending_content = None;
        self.rendering_content = None;
        self.submitted_content = None;
        self.presented_content = None;
        self.submitted_group_frame = None;
        self.prepared_group_frame = None;
        self.submitted_at = None;
        self.submitted_ust_usec = None;
        self.output_frames = frames;
    }

    pub(super) fn queue_page_flip_callback(
        &mut self,
        callback: crate::LivePageFlipCallback,
    ) -> Result<bool, &'static str> {
        // The card pump normalizes the logical output before calling here;
        // keep the head boundary explicit for any other callback producer.
        if callback.head != self.head || callback.output != self.output.id {
            return Err("native callback does not name its physical head");
        }
        if self.completion_mode == LiveProductionKmsCompletionMode::OutFenceAuthoritative {
            self.late_page_flip_events = self.late_page_flip_events.saturating_add(1);
            return Ok(false);
        }
        if self.pending_callback.is_some() {
            return Err("native head completion ledger is full");
        }
        self.pending_callback = Some(callback);
        Ok(true)
    }

    pub(super) fn synthesize_out_fence_callback(&mut self) -> crate::LivePageFlipCallback {
        let serial = self
            .last_callback_serial
            .unwrap_or_default()
            .saturating_add(1);
        self.completion_mode = LiveProductionKmsCompletionMode::OutFenceAuthoritative;
        self.out_fence_retirements = self.out_fence_retirements.saturating_add(1);
        crate::LivePageFlipCallback {
            output: self.output.id,
            head: self.head,
            frame_serial: serial,
        }
    }
}

#[path = "../../../../tests/support/native_head_completion.rs"]
mod tests;
