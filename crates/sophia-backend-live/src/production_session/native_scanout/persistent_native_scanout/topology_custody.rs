use super::*;

struct TopologyCustodyHead<'a, D> {
    head: sophia_engine::RenderHeadId,
    output: OutputId,
    enabled: bool,
    custody: &'a mut crate::PersistentScanoutCustody,
    device: &'a D,
}

impl LiveProductionNativeScanout {
    pub(crate) fn singleton_custody_scope(
        &self,
        output: OutputId,
    ) -> Option<(crate::NativeFrameOwner, sophia_engine::RenderHeadId)> {
        let indices = self.head_indices(output);
        (indices.len() == 1).then(|| (self.native_frame_owner, self.heads[indices[0]].head))
    }

    /// Called after the blocking topology commit and all fallible runtime
    /// construction. Singleton flips use runtime custody; mirrors use head
    /// custody. Leaving a singleton's topology owner in the head ledger pins a
    /// second renderer slot after the first ordinary flip replaces it.
    pub(crate) fn handoff_installed_topology_custody(
        &mut self,
        previous: &mut crate::LiveProductionOutputRuntimeSet,
        next: &mut crate::LiveProductionOutputRuntimeSet,
    ) -> Result<(), &'static str> {
        let mut heads = self
            .heads
            .iter_mut()
            .map(|head| TopologyCustodyHead {
                head: head.head,
                output: head.output.id,
                enabled: head.enabled,
                custody: &mut head.scanout_custody,
                device: self.groups[head.group].session.card(),
            })
            .collect::<Vec<_>>();
        let cleanup =
            handoff_topology_custody(self.native_frame_owner, &mut heads, previous, next)?;
        self.output_topology_cleanup.extend(cleanup);
        Ok(())
    }
}

fn handoff_topology_custody<D: crate::LibdrmNativePrimaryPlaneResourceDevice>(
    native_owner: crate::NativeFrameOwner,
    heads: &mut [TopologyCustodyHead<'_, D>],
    previous: &mut crate::LiveProductionOutputRuntimeSet,
    next: &mut crate::LiveProductionOutputRuntimeSet,
) -> Result<
    Vec<(
        sophia_engine::RenderHeadId,
        crate::BoxedRenderedPrimaryPlaneScanoutCleanup,
    )>,
    &'static str,
> {
    // Validate the entire handoff before moving any affine owner. Physical
    // head identities survive regrouping and disabled outputs; output IDs
    // and connector numbers alone cannot route old owners across cards.
    // Runtime-set iteration is stable (BTreeMap by output), and neither
    // pass changes membership. Each prevalidated route names the same cell.
    let mut previous_heads = Vec::new();
    for output in previous.values() {
        let runtime = output.runtime.primary_output_state();
        let custody = &runtime.scanout_custody;
        if custody.submitted().is_some() || custody.cleanup_pending() {
            return Err("topology handoff requires quiescent runtime custody");
        }
        let index = match custody.displayed() {
            None => None,
            Some(owner) => {
                let (scope_owner, scope_head) = runtime
                    .native_custody_scope
                    .ok_or("topology handoff lost the old displayed head scope")?;
                if scope_owner != native_owner {
                    return Err("topology handoff found a foreign displayed owner");
                }
                if let Some(frame) = owner.correlation().and_then(|frame| frame.native)
                    && (frame.owner() != scope_owner.raw() || frame.head() != scope_head)
                {
                    return Err("topology handoff displayed frame disagrees with its head scope");
                }
                Some(
                    heads
                        .iter()
                        .position(|head| head.head == scope_head)
                        .ok_or("topology handoff lost the old displayed head")?,
                )
            }
        };
        previous_heads.push(index);
    }
    let mut singleton_heads = Vec::new();
    for output in next.values() {
        let runtime = output.runtime.primary_output_state();
        let indices = heads
            .iter()
            .enumerate()
            .filter(|(_, head)| head.enabled && head.output == runtime.output)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if indices.is_empty() {
            return Err("topology handoff output has no enabled head");
        }
        let index = (indices.len() == 1).then(|| indices[0]);
        if let Some(index) = index
            && (!heads[index]
                .custody
                .can_transfer_displayed_to(&runtime.scanout_custody)
                || !runtime.retain_rendered_primary_plane_displayed_submission
                || runtime.native_custody_scope != Some((native_owner, heads[index].head)))
        {
            return Err("topology handoff cannot adopt the displayed singleton owner");
        }
        singleton_heads.push(index);
    }

    // The old images are off-plane after the completed blocking commit.
    // Failed rmfb/blob cleanup leaves native custody before old runtimes
    // disappear, and continues to block another topology preparation.
    let mut cleanup = Vec::new();
    for (output, index) in previous.values_mut().zip(previous_heads) {
        if let Some(index) = index
            && let Some(pending) = output
                .runtime
                .primary_output_state_mut()
                .scanout_custody
                .retire_replaced_displayed(heads[index].device)
                .expect("old runtime custody prevalidated before handoff")
        {
            cleanup.push((heads[index].head, pending));
        }
    }
    for (output, index) in next.values_mut().zip(singleton_heads) {
        if let Some(index) = index {
            assert!(
                heads[index].custody.transfer_displayed_to(
                    &mut output.runtime.primary_output_state_mut().scanout_custody
                ),
                "singleton custody prevalidated before handoff"
            );
        }
    }
    Ok(cleanup)
}

#[cfg(test)]
#[path = "../../../../tests/support/output_topology_handoff.rs"]
mod tests;
