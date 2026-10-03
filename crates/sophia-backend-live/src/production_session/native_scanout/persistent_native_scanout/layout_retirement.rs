use super::*;

/// A layout comparison whose unchanged alternative completed on this physical head.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionRetiredLayoutWitness {
    pub witness: crate::LiveScanoutLayoutWitness,
    pub device: LiveRenderDeviceNodeIdentity,
    pub head: sophia_engine::RenderHeadId,
    pub target_generation: u64,
    pub context_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LiveProductionNativeRetirementContent {
    pub content: LiveProductionScanoutContent,
    pub submission: u64,
    pub direct: bool,
    pub layout_witness: Option<LiveProductionRetiredLayoutWitness>,
    pub clocks: crate::LiveNativeRetirementClocks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LayoutWitnessContext {
    device: LiveRenderDeviceNodeIdentity,
    output: OutputId,
    head: sophia_engine::RenderHeadId,
    target_generation: u64,
    context_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SubmittedLayoutWitness {
    context: LayoutWitnessContext,
    cycle: u64,
    frame: LiveProductionNativeFrameId,
    witness: crate::LiveScanoutLayoutWitness,
}

#[derive(Default)]
pub(super) struct NativeLayoutWitnessState {
    submitted: Option<SubmittedLayoutWitness>,
    completed: Option<SubmittedLayoutWitness>,
}

impl NativeLayoutWitnessState {
    fn submit(
        &mut self,
        context: Option<LayoutWitnessContext>,
        cycle: u64,
        content: Option<LiveProductionScanoutContent>,
        witness: Option<crate::LiveScanoutLayoutWitness>,
    ) {
        self.submitted = None;
        let (Some(context), Some(content), Some(witness)) = (context, content, witness) else {
            return;
        };
        if witness
            .alternative
            .trace
            .is_none_or(|trace| trace.output != context.output || trace.head != context.head)
        {
            return;
        }
        self.submitted = Some(SubmittedLayoutWitness {
            context,
            cycle,
            frame: content.frame(),
            witness,
        });
    }

    fn retire(
        &mut self,
        context: Option<LayoutWitnessContext>,
        cycle: Option<u64>,
        content: Option<LiveProductionScanoutContent>,
        report: crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireReport,
    ) {
        use crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus as Status;
        match report.status {
            Status::HeadLost => self.invalidate(),
            Status::NoSubmission | Status::WaitingForAcceptedPageFlip => {}
            Status::RetiredAfterPageFlip | Status::ResourceRetireFailed => {
                self.completed = None;
                let Some(submitted) = self.submitted.take() else {
                    return;
                };
                if Some(submitted.context) == context
                    && Some(submitted.cycle) == cycle
                    && Some(submitted.frame) == content.map(LiveProductionScanoutContent::frame)
                    && Some(submitted.witness) == report.layout_witness
                {
                    self.completed = Some(submitted);
                }
            }
        }
    }

    fn take_completed(
        &mut self,
        context: Option<LayoutWitnessContext>,
        cycle: u64,
        frame: LiveProductionNativeFrameId,
    ) -> Option<LiveProductionRetiredLayoutWitness> {
        let completed = self.completed.take()?;
        (Some(completed.context) == context && completed.cycle == cycle && completed.frame == frame)
            .then_some(LiveProductionRetiredLayoutWitness {
                witness: completed.witness,
                device: completed.context.device,
                head: completed.context.head,
                target_generation: completed.context.target_generation,
                context_generation: completed.context.context_generation,
            })
    }

    pub(super) fn invalidate(&mut self) {
        self.submitted = None;
        self.completed = None;
    }
}

impl LiveProductionNativeScanout {
    fn layout_witness_context(&self, index: usize) -> Option<LayoutWitnessContext> {
        let head = self.heads.get(index)?;
        let (context, device) = self.output_allocation_context(head.output.id)?;
        if context.head != head.head {
            return None;
        }
        Some(LayoutWitnessContext {
            device,
            output: head.output.id,
            head: context.head,
            target_generation: context.target_generation,
            context_generation: context.generation,
        })
    }

    pub(super) fn observe_layout_witness_submit(
        &mut self,
        index: usize,
        report: crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport,
    ) {
        let context = self.layout_witness_context(index);
        let head = &mut self.heads[index];
        let witness = (!head.submitted_direct && !report.cursor_dropped)
            .then_some(report.layout_witness)
            .flatten();
        head.layout_witness.submit(
            context,
            u64::try_from(head.submissions).unwrap_or(u64::MAX),
            head.submitted_content,
            witness,
        );
    }

    pub(super) fn observe_layout_witness_retire(
        &mut self,
        index: usize,
        report: crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireReport,
    ) {
        let context = self.layout_witness_context(index);
        let head = &mut self.heads[index];
        head.layout_witness.retire(
            context,
            head.submitted_sequence
                .and_then(|cycle| u64::try_from(cycle).ok()),
            head.submitted_content,
            report,
        );
    }

    pub(super) fn completed_native_content(
        &mut self,
        index: usize,
    ) -> Option<LiveProductionNativeRetirementContent> {
        let context = self.layout_witness_context(index);
        let head = &mut self.heads[index];
        let content = head.presented_content?;
        let layout_witness = head.layout_witness.take_completed(
            context,
            u64::try_from(head.presented_submissions).ok()?,
            content.frame(),
        );
        Some(LiveProductionNativeRetirementContent {
            content,
            submission: u64::try_from(head.presented_submissions).ok()?,
            direct: head.presented_direct,
            layout_witness,
            clocks: crate::LiveNativeRetirementClocks::from_evidence(
                head.presented_completion_timestamp
                    .and_then(|timestamp| timestamp.clock),
            ),
        })
    }
}

#[path = "../../../../tests/support/native_layout_retirement.rs"]
mod tests;
