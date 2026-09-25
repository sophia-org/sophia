use std::collections::BTreeMap;

use sophia_protocol::{
    OutputId, POLICY_MAX_PRESENTATION_OUTPUTS, PolicyPresentation, PolicyPresentationIdentity,
    PolicyPresentationOutcome, PolicyPresentationReceipt, WmActionId, WmModifierMask,
};

mod capture;
pub use capture::*;

/// Attestation read from a completed output frame, never from requested state.
/// Source content generations deliberately do not participate in this identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyPresentationCompletion {
    pub owner_epoch: u64,
    pub publication_generation: u64,
    pub output: OutputId,
    pub output_generation: u64,
}

/// Engine's input authority for an already validated WM publication.
///
/// Transport owns delivery of returned receipts. Neither a full transport queue
/// nor a peer acknowledgment can delay revocation or restore this state.
#[derive(Debug, Default)]
pub struct PresentedPolicyState {
    owner_epoch: u64,
    greatest_generation: u64,
    next_presentation_epoch: u64,
    publication: Option<PolicyPresentation>,
    completed: BTreeMap<OutputId, PolicyPresentationReceipt>,
}

impl PresentedPolicyState {
    pub fn pointer_hit(&self, output: OutputId, point: sophia_protocol::Point) -> PolicyPointerHit {
        let Some(publication) = &self.publication else {
            return PolicyPointerHit::Pass;
        };
        let Some(receipt) = self.output_receipt(output) else {
            return PolicyPointerHit::Pass;
        };
        let Some(scope) = publication
            .outputs
            .iter()
            .find(|scope| scope.output == output)
        else {
            return PolicyPointerHit::Pass;
        };
        if !contains(scope.coverage, point) {
            return PolicyPointerHit::Pass;
        }
        if publication.keyboard_output.is_some() && !self.modal_ready(false) {
            return PolicyPointerHit::Blocked;
        }
        let hit = publication
            .instances
            .iter()
            .filter(|i| {
                i.output == output && contains(i.destination, point) && contains(i.clip, point)
            })
            .map(|i| (i.z_index, i.id, i.generation, i.action))
            .chain(
                publication
                    .regions
                    .iter()
                    .filter(|r| {
                        r.output == output && contains(r.geometry, point) && contains(r.clip, point)
                    })
                    .map(|r| (r.z_index, r.id, r.generation, r.action)),
            )
            .max_by_key(|target| target.0);
        match hit {
            Some((_, target, generation, Some(action))) => {
                PolicyPointerHit::Action(PresentedPolicyAction {
                    connection_epoch: receipt.connection_epoch,
                    action,
                    identity: identity(receipt, target, generation),
                })
            }
            Some(_) => PolicyPointerHit::Blocked,
            None if scope.mode == sophia_protocol::PolicyPresentationMode::ReplaceApplications => {
                PolicyPointerHit::Blocked
            }
            None => PolicyPointerHit::Pass,
        }
    }
    /// Install admitted immutable records. The caller must retain the returned
    /// revocations separately from action credit. Admission is not presentation.
    pub fn admit(
        &mut self,
        owner_epoch: u64,
        publication: PolicyPresentation,
    ) -> Result<Vec<PolicyPresentationReceipt>, &'static str> {
        if owner_epoch == self.owner_epoch && self.publication.as_ref() == Some(&publication) {
            return Ok(Vec::new());
        }
        if owner_epoch == 0
            || owner_epoch < self.owner_epoch
            || publication.generation == 0
            || (owner_epoch == self.owner_epoch
                && publication.generation <= self.greatest_generation)
            || publication.outputs.is_empty()
            || publication.outputs.len() > POLICY_MAX_PRESENTATION_OUTPUTS
        {
            return Err("stale or unbounded presentation admission");
        }
        let revoked = self.revoke();
        self.owner_epoch = owner_epoch;
        self.greatest_generation = publication.generation;
        self.publication = Some(publication);
        Ok(revoked)
    }

    pub fn publication(&self) -> Option<(u64, &PolicyPresentation)> {
        Some((self.owner_epoch, self.publication.as_ref()?))
    }

    /// Issue exactly one receipt per interaction publication and output. A
    /// content repaint preserves the original receipt and pointer identity.
    pub fn complete(
        &mut self,
        completion: PolicyPresentationCompletion,
    ) -> Option<PolicyPresentationReceipt> {
        let publication = self.publication.as_ref()?;
        if completion.owner_epoch != self.owner_epoch
            || completion.publication_generation != publication.generation
            || !publication.outputs.iter().any(|output| {
                output.output == completion.output
                    && output.generation == completion.output_generation
            })
            || self.completed.contains_key(&completion.output)
        {
            return None;
        }
        // Exhaustion fails closed; an epoch must never alias an old receipt.
        self.next_presentation_epoch = self.next_presentation_epoch.checked_add(1)?;
        let receipt = PolicyPresentationReceipt {
            connection_epoch: completion.owner_epoch,
            publication_generation: completion.publication_generation,
            output: completion.output,
            output_generation: completion.output_generation,
            presentation_epoch: self.next_presentation_epoch,
            outcome: PolicyPresentationOutcome::Presented,
        };
        self.completed.insert(completion.output, receipt);
        Some(receipt)
    }

    pub fn output_receipt(&self, output: OutputId) -> Option<PolicyPresentationReceipt> {
        self.completed.get(&output).copied()
    }

    /// Existing application sequences retain their owner until settlement.
    pub fn modal_ready(&self, application_capture_active: bool) -> bool {
        !application_capture_active
            && self.publication.as_ref().is_some_and(|publication| {
                publication.keyboard_output.is_some()
                    && publication
                        .outputs
                        .iter()
                        .all(|output| self.completed.contains_key(&output.output))
            })
    }

    pub fn actions_registered(&self, actions: &[WmActionId]) -> bool {
        self.publication.as_ref().is_none_or(|publication| {
            publication
                .bindings
                .iter()
                .map(|binding| binding.action)
                .chain(
                    publication
                        .instances
                        .iter()
                        .filter_map(|instance| instance.action),
                )
                .chain(
                    publication
                        .regions
                        .iter()
                        .filter_map(|region| region.action),
                )
                .all(|action| actions.contains(&action))
        })
    }

    /// Profile/catalog replacement revokes the entire publication if any of
    /// its actions lost admission. Session-operation tokens are never passed
    /// in this list by the session's pure-action registry.
    pub fn revalidate_actions(&mut self, actions: &[WmActionId]) -> Vec<PolicyPresentationReceipt> {
        if self.actions_registered(actions) {
            Vec::new()
        } else {
            self.revoke()
        }
    }

    pub fn keyboard_action(
        &self,
        keycode: u32,
        modifiers: WmModifierMask,
        application_capture_active: bool,
    ) -> Option<(WmActionId, PolicyPresentationIdentity)> {
        if !self.modal_ready(application_capture_active) {
            return None;
        }
        let publication = self.publication.as_ref()?;
        let binding = publication
            .bindings
            .iter()
            .find(|binding| binding.keycode == keycode && binding.modifiers == modifiers)?;
        let receipt = self.output_receipt(publication.keyboard_output?)?;
        Some((binding.action, identity(receipt, 0, 0)))
    }

    /// Recheck an action immediately before issuance and again on its reply.
    /// A matching reducer record alone does not prove completed input authority.
    pub fn action_is_current(
        &self,
        owner_epoch: u64,
        action: WmActionId,
        identity: PolicyPresentationIdentity,
    ) -> bool {
        let Some(publication) = &self.publication else {
            return false;
        };
        let Some(receipt) = self.output_receipt(identity.output) else {
            return false;
        };
        if owner_epoch != receipt.connection_epoch
            || identity.publication_generation != receipt.publication_generation
            || identity.output_generation != receipt.output_generation
            || identity.presentation_epoch != receipt.presentation_epoch
        {
            return false;
        }
        if identity.target_id == 0 {
            return identity.target_generation == 0
                && self.modal_ready(false)
                && publication.keyboard_output == Some(identity.output)
                && publication
                    .bindings
                    .iter()
                    .any(|binding| binding.action == action);
        }
        if publication.keyboard_output.is_some() && !self.modal_ready(false) {
            return false;
        }
        publication.instances.iter().any(|instance| {
            instance.output == identity.output
                && instance.id == identity.target_id
                && instance.generation == identity.target_generation
                && instance.action == Some(action)
        }) || publication.regions.iter().any(|region| {
            region.output == identity.output
                && region.id == identity.target_id
                && region.generation == identity.target_generation
                && region.action == Some(action)
        })
    }

    /// Receipts exist only for outputs which actually presented this identity.
    /// The high-water marks survive withdrawal, rejecting late resurrection.
    pub fn revoke(&mut self) -> Vec<PolicyPresentationReceipt> {
        self.publication = None;
        std::mem::take(&mut self.completed)
            .into_values()
            .map(|mut receipt| {
                receipt.outcome = PolicyPresentationOutcome::Revoked;
                receipt
            })
            .collect()
    }
}

fn identity(
    receipt: PolicyPresentationReceipt,
    target_id: u64,
    target_generation: u64,
) -> PolicyPresentationIdentity {
    PolicyPresentationIdentity {
        publication_generation: receipt.publication_generation,
        output: receipt.output,
        output_generation: receipt.output_generation,
        presentation_epoch: receipt.presentation_epoch,
        target_id,
        target_generation,
    }
}

fn contains(rect: sophia_protocol::Rect, point: sophia_protocol::Point) -> bool {
    point.x.is_finite()
        && point.y.is_finite()
        && !rect.is_empty()
        && point.x >= f64::from(rect.x)
        && point.y >= f64::from(rect.y)
        && point.x < f64::from(rect.x) + f64::from(rect.width)
        && point.y < f64::from(rect.y) + f64::from(rect.height)
}
