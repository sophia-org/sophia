//! Desired configuration, speculative hardware replacement, and published
//! realization have separate lifetimes. Waiting never erases committed policy.

use sophia_backend_live::LibdrmNativeOutputCapability;
use sophia_config::{DesktopOutputCandidate, DesktopOutputReconciliation};
use sophia_protocol::OutputId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OutputRealizationBinding {
    pub transition: u64,
    pub notice_sequence: u64,
    pub native_owner: u64,
}

struct PendingRealization {
    binding: OutputRealizationBinding,
    realization: DesktopOutputReconciliation,
}

#[derive(Default)]
pub(super) struct OutputRealizationLedger {
    committed: Option<DesktopOutputReconciliation>,
    pending: Option<PendingRealization>,
}

impl OutputRealizationLedger {
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

#[path = "../../tests/support/output_realization.rs"]
mod tests;
