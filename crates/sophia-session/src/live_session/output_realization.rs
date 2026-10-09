//! Desired configuration, speculative hardware replacement, and published
//! realization have separate lifetimes. Waiting never erases committed policy.

use sophia_backend_live::LibdrmNativeOutputCapability;
use sophia_config::{DesktopOutputCandidate, DesktopOutputReconciliation};
use sophia_protocol::OutputId;

mod layout;
pub(super) use layout::OutputPolicyLayout;
pub(super) use layout::matches_presented;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OutputRealizationBinding {
    pub transition: u64,
    pub notice_sequence: u64,
    pub native_owner: u64,
}

pub(super) struct PendingOutputPublication {
    pub binding: OutputRealizationBinding,
    pub snapshot: sophia_protocol::OutputAuthoritySnapshot,
    pub capabilities: Vec<LibdrmNativeOutputCapability>,
    pub already_published: bool,
}

struct PendingRealization {
    binding: OutputRealizationBinding,
    realization: DesktopOutputReconciliation,
}

#[derive(Default)]
pub(super) struct OutputRealizationLedger {
    committed: Option<DesktopOutputReconciliation>,
    pending: Option<PendingRealization>,
    policy: Option<(sophia_protocol::TransactionId, DesktopOutputReconciliation)>,
}

impl OutputRealizationLedger {
    pub fn prepare_policy(
        &mut self,
        transaction: sophia_protocol::TransactionId,
        realization: DesktopOutputReconciliation,
    ) {
        self.policy = Some((transaction, realization));
    }

    pub fn take_policy(
        &mut self,
        transaction: sophia_protocol::TransactionId,
        profile: &DesktopOutputCandidate,
    ) -> Option<DesktopOutputReconciliation> {
        if self
            .policy
            .as_ref()
            .is_none_or(|(pending, _)| *pending != transaction)
        {
            return None;
        }
        self.policy
            .take()
            .map(|(_, realization)| realization)
            .filter(|realization| {
                realization.generation == profile.generation && realization.digest == profile.digest
            })
    }

    pub fn committed(&self) -> Option<&DesktopOutputReconciliation> {
        self.committed.as_ref()
    }

    pub fn stage(
        &mut self,
        binding: OutputRealizationBinding,
        realization: DesktopOutputReconciliation,
    ) -> Result<(), &'static str> {
        if binding.native_owner == 0 {
            return Err("output realization has no native owner");
        }
        if let Some(pending) = &self.pending {
            if binding.transition < pending.binding.transition
                || binding.notice_sequence < pending.binding.notice_sequence
            {
                return Err("output realization is older than the pending replacement");
            }
            if binding == pending.binding && realization != pending.realization {
                return Err("one replacement cannot carry two output realizations");
            }
        }
        record_resolution("resolved", binding, &realization);
        self.pending = Some(PendingRealization {
            binding,
            realization,
        });
        Ok(())
    }

    pub fn pending(
        &self,
        binding: OutputRealizationBinding,
        profile: &DesktopOutputCandidate,
    ) -> Option<&DesktopOutputReconciliation> {
        self.pending
            .as_ref()
            .filter(|pending| {
                pending.binding == binding
                    && pending.realization.generation == profile.generation
                    && pending.realization.digest == profile.digest
            })
            .map(|pending| &pending.realization)
    }

    /// Call only at the existing presented-topology publication barrier. A late
    /// completion cannot consume a newer pending replacement, even if logical
    /// output numbers or the transition number were reused during quarantine.
    pub fn commit(
        &mut self,
        binding: OutputRealizationBinding,
        profile: &DesktopOutputCandidate,
    ) -> bool {
        if self.pending(binding, profile).is_none() {
            return false;
        }
        self.committed = self.pending.take().map(|pending| pending.realization);
        if let Some(realization) = &self.committed {
            record_resolution("committed", binding, realization);
        }
        true
    }

    pub fn abandon(&mut self) {
        self.pending = None;
    }

    /// Focus is live WM state, not the saved focus-at-startup preference. Call
    /// while the committed owner's capability mapping is still available.
    pub fn observe_focus(
        &mut self,
        focused: OutputId,
        capabilities: &[LibdrmNativeOutputCapability],
    ) {
        let Some(committed) = &mut self.committed else {
            return;
        };
        if let Some(output) = committed.outputs.iter().find(|output| {
            output.enabled
                && output.mirror_of.is_none()
                && capabilities.iter().any(|capability| {
                    capability.output() == focused && capability.connector_key() == output.connector
                })
        }) {
            committed.focused_connector = Some(output.connector.clone());
        }
    }
}

fn record_resolution(
    status: &str,
    binding: OutputRealizationBinding,
    realization: &DesktopOutputReconciliation,
) {
    // Both rescan and policy admission increment the owner transition before
    // staging. Zero belongs only to the initial startup owner.
    let phase = if binding.transition == 0 {
        "startup"
    } else {
        "runtime"
    };
    tracing::info!(target: "sophia_scanout_evidence",
        "sophia_live_output_resolution schema=1 phase={phase} status={status} generation={} transition={} notice={} owner={} outputs={} adjustments={}",
        realization.generation.raw(), binding.transition, binding.notice_sequence, binding.native_owner,
        realization.outputs.iter().filter(|state| state.enabled && state.mirror_of.is_none()).count(), realization.adjustments.len());
    // One bounded batch per staged realization, never per frame or idle pass.
    if status == "resolved" {
        for adjustment in realization.adjustments.iter().take(128) {
            tracing::info!(target: "sophia_scanout_evidence",
                "sophia_live_output_adjustment schema=1 phase={phase} reason={:?}", adjustment.reason);
        }
    }
}

#[path = "../../tests/support/output_realization.rs"]
mod tests;
