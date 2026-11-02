//! W2 prototype: a candidate that names one predecessor to retire once it is
//! presented. Only a connection that negotiated revision 12 with
//! capability bit 17 granted may admit one; nothing else changes. The intent
//! is encoded into the candidate's own file record, so every retry, refusal
//! and revocation of that record carries or drops it with the candidate.
//!
//! [`RetireOnPresent`] follows each named predecessor in wire order, as
//! `ContentLifecycle` follows candidates. It performs no I/O and sends no
//! explicit retire: while a predecessor is held, the client must neither
//! retire nor reuse it, and only its exact `ResourceReleased` frees it.
use std::collections::BTreeMap;

use sophia_shell_protocol::SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION;
use sophia_shell_protocol::shell::content::{
    ContentCandidateRetire, SOPHIA_SHELL_CAPABILITY_CONTENT_RETIRE_ON_PRESENT,
    SOPHIA_SHELL_RETIRE_ON_PRESENT_REVISION, owned_ring_image_revision, retire_on_present_enabled,
};
use sophia_shell_protocol::shell_files::ShellFileNegotiated;
use sophia_shell_protocol::{
    ContentGrant, ContentOutputId, ContentResourceId, ShellContentRecord, TransactionId,
};

use crate::wire::Outbound;
use crate::{Admission, ShellClientError, ShellClientOptions, ShellConnection};

impl ShellConnection {
    /// Whether the negotiated welcome permits a retire intent.
    pub fn retire_on_present(&self) -> bool {
        retire_on_present_enabled(self.welcome.selected_revision, self.welcome.capabilities)
    }

    /// The owned-ring image protocol revision this welcome carries, if any.
    pub(crate) fn image_revision(&self) -> Option<u16> {
        owned_ring_image_revision(self.welcome.selected_revision, self.welcome.capabilities)
    }

    /// [`Self::enqueue_content_group`] carrying `retire`. Without the
    /// negotiated permission, or with an intent the session would refuse
    /// outright, nothing is queued and no submission ID is spent.
    pub fn enqueue_content_group_with_retire(
        &mut self,
        transaction: TransactionId,
        records: &[ShellContentRecord],
        retire: ContentCandidateRetire,
    ) -> Result<(), ShellClientError> {
        self.enqueue_content_group_with_retire_tracked(transaction, records, retire)
            .map(|_| ())
    }

    /// [`Self::enqueue_content_group_with_retire`] with its ticket.
    pub fn enqueue_content_group_with_retire_tracked(
        &mut self,
        transaction: TransactionId,
        records: &[ShellContentRecord],
        retire: ContentCandidateRetire,
    ) -> Result<Admission, ShellClientError> {
        if !self.retire_on_present() {
            return Err(ShellClientError::MissingCapability);
        }
        let draws_it = records.iter().any(|record| {
            matches!(record, ShellContentRecord::CandidateChunk(chunk)
                if chunk.placements.iter().any(|p| p.resource == retire.resource))
        });
        if !retire.retire_transaction.is_valid()
            || retire.retire_transaction == transaction
            || retire.resource.id == 0
            || retire.resource.generation == 0
            || draws_it
        {
            return Err(ShellClientError::InvalidArgument("retire intent"));
        }
        self.admit(
            Outbound::RetiringContentGroup(transaction, records.to_vec(), retire),
            false,
        )
    }
}

pub(crate) fn w2_required(options: &ShellClientOptions) -> bool {
    options.required_capabilities & SOPHIA_SHELL_CAPABILITY_CONTENT_RETIRE_ON_PRESENT != 0
}

/// Both handshakes' check of a W2 welcome: one that requires bit 17 gets
/// exactly r12 and exactly what it required, and nothing grants bit 17
/// unasked. The ordinary range and superset checks run first.
pub(crate) fn validate_w2_welcome(
    options: &ShellClientOptions,
    negotiated: &ShellFileNegotiated,
) -> Result<(), ShellClientError> {
    let welcome = &negotiated.welcome;
    let granted = welcome.capabilities & SOPHIA_SHELL_CAPABILITY_CONTENT_RETIRE_ON_PRESENT != 0;
    if !w2_required(options) {
        return if granted {
            Err(ShellClientError::MissingCapability)
        } else {
            Ok(())
        };
    }
    if welcome.selected_revision != SOPHIA_SHELL_RETIRE_ON_PRESENT_REVISION {
        return Err(ShellClientError::UnsupportedRevision);
    }
    if welcome.capabilities
        != options.required_capabilities | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
    {
        return Err(ShellClientError::MissingCapability);
    }
    Ok(())
}

/// What one observed record did to a named predecessor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetireProgress {
    /// No named predecessor changed.
    Unrelated,
    /// Its candidate was presented: the session retires it, and its exact
    /// `ResourceReleased` follows.
    Retiring(ContentResourceId),
    /// Its candidate ended without presentation: the predecessor is still
    /// the client's, as if it had never been named.
    Kept(ContentResourceId),
    /// Released under its retire transaction: its ID may be reused.
    Released(ContentResourceId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetireOnPresentError {
    WrongGrant,
    /// The candidate, resource or retire transaction is already named.
    Duplicate,
    /// A release or outcome that contradicts a named predecessor.
    Misattributed,
}

struct Named {
    output: ContentOutputId,
    retire: ContentCandidateRetire,
}

/// Predecessors named by admitted candidates, in wire order.
pub struct RetireOnPresent {
    grant: ContentGrant,
    named: BTreeMap<u64, Named>,
    retiring: Vec<ContentCandidateRetire>,
}

impl RetireOnPresent {
    pub fn new(grant: ContentGrant) -> Self {
        Self {
            grant,
            named: BTreeMap::new(),
            retiring: Vec::new(),
        }
    }

    /// Record candidate `candidate_generation`'s intent once it is admitted.
    pub fn name(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
        retire: ContentCandidateRetire,
    ) -> Result<(), RetireOnPresentError> {
        let taken = |held: &ContentCandidateRetire| {
            held.resource == retire.resource || held.retire_transaction == retire.retire_transaction
        };
        if self.named.contains_key(&candidate_generation)
            || self.named.values().any(|named| taken(&named.retire))
            || self.retiring.iter().any(taken)
        {
            return Err(RetireOnPresentError::Duplicate);
        }
        self.named
            .insert(candidate_generation, Named { output, retire });
        Ok(())
    }

    /// Whether `resource` is named or retiring: neither an explicit retire
    /// nor a reuse of its identity is allowed yet.
    pub fn holds(&self, resource: ContentResourceId) -> bool {
        self.named.values().any(|n| n.retire.resource == resource)
            || self.retiring.iter().any(|r| r.resource == resource)
    }

    /// Observe one session record, in wire order.
    pub fn observe(
        &mut self,
        transaction: TransactionId,
        record: &ShellContentRecord,
    ) -> Result<RetireProgress, RetireOnPresentError> {
        match record {
            ShellContentRecord::CandidateOutcome(outcome) => {
                if outcome.grant != self.grant {
                    return Err(RetireOnPresentError::WrongGrant);
                }
                let Some(named) = self.named.get(&outcome.candidate_generation) else {
                    return Ok(RetireProgress::Unrelated);
                };
                if named.output != outcome.output {
                    return Err(RetireOnPresentError::Misattributed);
                }
                // Prepared is not terminal; the intent stays named.
                if outcome.kind == 1 {
                    return Ok(RetireProgress::Unrelated);
                }
                let named = self
                    .named
                    .remove(&outcome.candidate_generation)
                    .expect("found above");
                if outcome.kind == 2 {
                    self.retiring.push(named.retire);
                    Ok(RetireProgress::Retiring(named.retire.resource))
                } else {
                    Ok(RetireProgress::Kept(named.retire.resource))
                }
            }
            ShellContentRecord::ResourceReleased(released) => {
                if released.grant != self.grant {
                    return Err(RetireOnPresentError::WrongGrant);
                }
                let at = self.retiring.iter().position(|r| {
                    r.retire_transaction == transaction || r.resource == released.resource
                });
                if let Some(at) = at {
                    let retire = self.retiring[at];
                    if retire.retire_transaction != transaction
                        || retire.resource != released.resource
                    {
                        return Err(RetireOnPresentError::Misattributed);
                    }
                    self.retiring.swap_remove(at);
                    return Ok(RetireProgress::Released(retire.resource));
                }
                // Before Presented a named predecessor cannot be released.
                if self.named.values().any(|n| {
                    n.retire.resource == released.resource
                        || n.retire.retire_transaction == transaction
                }) {
                    return Err(RetireOnPresentError::Misattributed);
                }
                Ok(RetireProgress::Unrelated)
            }
            _ => Ok(RetireProgress::Unrelated),
        }
    }

    /// The grant ended: every named or retiring predecessor ended with it.
    pub fn revoke(&mut self) {
        self.named.clear();
        self.retiring.clear();
    }
}
