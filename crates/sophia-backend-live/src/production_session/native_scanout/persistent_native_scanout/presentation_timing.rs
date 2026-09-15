//! Diagnostic witness from exact displayed custody, never log-observation time.
use super::*;

pub(crate) struct PresentedTimingHead {
    pub expected: crate::LiveNativeFrameIdentity,
    pub displayed: Option<crate::LiveNativeFrameIdentity>,
    pub completed_usec: u64,
    pub kernel_timestamp: bool,
    pub missing_kernel_timestamp: bool,
}

pub(crate) fn completed_timing(
    owner: crate::NativeFrameOwner,
    output: OutputId,
    frame: LiveProductionNativeFrameId,
    count: usize,
    group: Option<&LiveProductionMirrorGroupLifecycle>,
    heads: impl IntoIterator<Item = PresentedTimingHead>,
) -> Option<(u64, bool, bool)> {
    if !output.is_valid()
        || frame.raw() == 0
        || !(1..=sophia_engine::MAX_HEADS_PER_OUTPUT).contains(&count)
    {
        return None;
    }
    if count > 1
        && group.is_none_or(|g| {
            g.output() != output || g.failed() || !g.converged() || g.heads().count() != count
        })
    {
        return None;
    }
    let mut seen = [None; sophia_engine::MAX_HEADS_PER_OUTPUT];
    let mut visited = 0;
    let mut completed = 0;
    let mut all_kernel = true;
    let mut missing_kernel = false;
    for head in heads {
        if visited >= count
            || !head.expected.head().is_valid()
            || head.completed_usec == 0
            || seen[..visited].contains(&Some(head.expected.head()))
            || head.expected
                != owner.frame(
                    output,
                    head.expected.head(),
                    head.expected.target_generation(),
                    frame.raw(),
                )
            || head.displayed != Some(head.expected)
            || (count > 1 && group?.displayed_frame(head.expected.head()) != Some(frame))
        {
            return None;
        }
        completed = completed.max(head.completed_usec);
        all_kernel &= head.kernel_timestamp;
        missing_kernel |= head.missing_kernel_timestamp;
        seen[visited] = Some(head.expected.head());
        visited += 1;
    }
    (visited == count).then_some((completed, all_kernel, missing_kernel))
}

impl LiveProductionNativeScanout {
    pub(super) fn trace_shell_native_completion(
        &self,
        output: OutputId,
        frame: LiveProductionNativeFrameId,
    ) {
        let indices = self.head_indices(output);
        let Some((completed, all_kernel, missing_kernel)) = completed_timing(
            self.native_frame_owner,
            output,
            frame,
            indices.len(),
            self.output_lifecycles.get(&output),
            indices.iter().map(|index| {
                let head = &self.heads[*index];
                PresentedTimingHead {
                    expected: self.native_frame_owner.frame(
                        output,
                        head.head,
                        head.target_generation,
                        frame.raw(),
                    ),
                    displayed: head
                        .scanout_custody
                        .displayed()
                        .and_then(|owner| owner.correlation())
                        .and_then(|c| c.native),
                    completed_usec: head
                        .presented_completion_timestamp
                        .map_or(0, |t| t.ust_usec),
                    kernel_timestamp: head
                        .presented_completion_timestamp
                        .is_some_and(|t| t.used_kernel_timestamp),
                    missing_kernel_timestamp: head
                        .presented_completion_timestamp
                        .is_some_and(|t| t.missing_kernel_timestamp),
                }
            }),
        ) else {
            return;
        };
        let identity = self.native_frame_identity(indices[0], output, frame);
        tracing::info!(
                    target: "sophia_scanout_evidence",
            "sophia_shell_native_completion schema=1 output={} native_owner={} native_frame={} heads={} monotonic_usec={} timestamp_source={} missing_kernel_timestamp={}",
            output.raw(),
            identity.owner(),
            frame.raw(),
            indices.len(),
            completed,
            if all_kernel { "kernel" } else { "observation_fallback" },
            u8::from(missing_kernel),
        );
    }
}
