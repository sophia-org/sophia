#![cfg(test)]

impl<P> LiveBackendRuntimeAssembly<P>
where
    P: NonBlockingInputPoller,
{
    /// Drain physical-head callbacks without publishing a logical-output flip.
    ///
    /// A mirror group has one logical output but several independently flipping
    /// connectors. The group coordinator must join those callbacks before the
    /// Engine can observe `Presented`; the ordinary queue drain publishes each
    /// accepted callback immediately and is therefore only correct for one head.
    pub(crate) fn drain_mirror_page_flip_callback_queue(
        &mut self,
    ) -> LivePageFlipCallbackQueueReport {
        let Some(queue) = self.page_flip_callback_queue.take() else {
            return LivePageFlipCallbackQueueReport::default();
        };
        let report = queue.drain_ready_with(|callback| {
            let Some(state) = self.outputs.get_mut(callback.output) else {
                return LivePageFlipCallbackReport {
                    decision: LivePageFlipCallbackDecision::RejectedUnexpectedOutput,
                    event: LivePageFlipEvent {
                        status: LivePageFlipEventStatus::WaitingForOutput,
                        frame_serial: None,
                    },
                };
            };
            state.page_flip_callback_intake.observe(callback)
        });
        self.page_flip_callback_queue = Some(queue);
        report
    }
}

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/runtime_mirror_page_flip.rs"
));
