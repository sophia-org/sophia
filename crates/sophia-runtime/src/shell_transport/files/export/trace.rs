use super::*;

/// Comparator journal identities: which records answer which requests. The
/// 9P journal records sequences and byte ranges; these add the shell keys
/// that join them. Diagnostic only; nothing here changes custody.
pub(super) fn trace_key(
    class: &'static str,
    id: u64,
    generation: u64,
) -> sophia_9p::journal_trace::RoleKey {
    sophia_9p::journal_trace::RoleKey {
        class,
        id,
        generation,
    }
}

impl ShellFiles {
    /// The receipt for an accepted submission of `kind`, with the keys its
    /// answers may carry. Commands differ: some have no answer at all.
    pub(super) fn trace_submitted(
        &self,
        sequence: u64,
        submission_id: u64,
        kind: ShellFileKind,
        inbound: Option<&Inbound>,
    ) {
        use sophia_9p::journal_trace::RoleKey;
        let (transaction, key) = match inbound {
            Some(Inbound::Content(transaction, record)) => (
                transaction.raw(),
                match record.as_ref() {
                    ShellContentRecord::AllocationRequest(request) => {
                        trace_key("allocation_request", request.allocation_request_id, 0)
                    }
                    ShellContentRecord::ResourceCancel(cancel) => {
                        trace_key("resource", cancel.resource.id, cancel.resource.generation)
                    }
                    ShellContentRecord::ResourceRetire(retire) => {
                        trace_key("resource", retire.resource.id, retire.resource.generation)
                    }
                    _ => RoleKey::NONE,
                },
            ),
            Some(Inbound::Candidate(candidate)) => (
                candidate.transaction.raw(),
                trace_key(
                    "candidate_generation",
                    candidate.candidate.candidate_generation,
                    0,
                ),
            ),
            // Its resource status answers under the offer as transaction.
            Some(Inbound::ImageBegin(metadata)) => (
                metadata.offer_id,
                trace_key("resource", metadata.resource, metadata.resource_generation),
            ),
            Some(
                Inbound::NativeAllocation(transaction, _)
                | Inbound::NativeInputAck(transaction, _)
                | Inbound::NativeActivate(transaction, _)
                | Inbound::CatalogActivate(transaction, _)
                | Inbound::IndicatorActivate(transaction, _),
            ) => (transaction.raw(), RoleKey::NONE),
            _ => (0, RoleKey::NONE),
        };
        sophia_9p::journal_trace::role(sophia_9p::journal_trace::RoleRecord {
            journal: self.journal.trace_id(),
            sequence,
            phase: sophia_9p::journal_trace::RolePhase::Submitted,
            kind: kind as u16,
            transaction,
            key,
            status: 0,
            related: submission_id,
        });
    }

    /// An event answering a request, when its kind carries a join key. Other
    /// kinds are labeled by the journal wrapper alone.
    pub(super) fn trace_outcome(
        &self,
        sequence: u64,
        kind: ShellFileKind,
        body: &[u8],
        status: Option<(u64, sophia_9p::journal_trace::RoleKey, u16)>,
    ) {
        let identity = status.or_else(|| {
            if !matches!(
                kind,
                ShellFileKind::AllocationResult | ShellFileKind::CandidateOutcome
            ) {
                return None;
            }
            // Both are a transaction then a fixed value: decoded in place,
            // without building a record.
            let transaction = u64::from_le_bytes(body.get(..8)?.try_into().ok()?);
            let value_kind = if kind == ShellFileKind::AllocationResult {
                sophia_protocol::shell::encoding::content::ShellContentValueKind::AllocationResult
            } else {
                sophia_protocol::shell::encoding::content::ShellContentValueKind::CandidateOutcome
            };
            match sophia_protocol::shell::encoding::content::decode_shell_content_value(
                value_kind,
                &body[8..],
            )
            .ok()?
            {
                ShellContentRecord::AllocationResult(result) => Some((
                    transaction,
                    trace_key("allocation_request", result.allocation_request_id, 0),
                    result.status,
                )),
                ShellContentRecord::CandidateOutcome(outcome) => Some((
                    transaction,
                    trace_key("candidate_generation", outcome.candidate_generation, 0),
                    outcome.kind,
                )),
                _ => None,
            }
        });
        if let Some((transaction, key, status)) = identity {
            sophia_9p::journal_trace::role(sophia_9p::journal_trace::RoleRecord {
                journal: self.journal.trace_id(),
                sequence,
                phase: sophia_9p::journal_trace::RolePhase::Outcome,
                kind: kind as u16,
                transaction,
                key,
                status,
                related: 0,
            });
        }
    }

    /// The covering acknowledgement releases the accepted submission.
    pub(super) fn trace_released(&self, ack: u64) {
        if let Some(accepted) = &self.accepted {
            sophia_9p::journal_trace::role(sophia_9p::journal_trace::RoleRecord {
                journal: self.journal.trace_id(),
                sequence: accepted.sequence,
                phase: sophia_9p::journal_trace::RolePhase::Released,
                kind: ShellFileKind::Submitted as u16,
                transaction: 0,
                key: trace_key("submission", accepted.submit.submission_id, 0),
                status: 0,
                related: ack,
            });
        }
    }
}
