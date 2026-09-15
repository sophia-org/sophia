use crate::{ClientContentCandidate, ContentLifecycleError, MAX_QUEUED_FRAMES, ShellClientError};
use sophia_protocol::{ShellContentRecord, TransactionId};

/// Derive target metadata from the exact complete group that will be sent;
/// callers cannot register one set of targets while enqueueing another.
pub(crate) fn metadata(
    transaction: TransactionId,
    records: &[ShellContentRecord],
) -> Result<ClientContentCandidate, ShellClientError> {
    let invalid = || ShellClientError::Lifecycle(ContentLifecycleError::InvalidCandidate);
    if records.len() < 2 || records.len() > MAX_QUEUED_FRAMES / 2 {
        return Err(invalid());
    }
    let Some(ShellContentRecord::CandidateBegin(begin)) = records.first() else {
        return Err(invalid());
    };
    let Some(ShellContentRecord::CandidateEnd(end)) = records.last() else {
        return Err(invalid());
    };
    if begin.grant != end.grant
        || begin.candidate_generation != end.candidate_generation
        || begin.surface_count != end.surface_count
        || begin.placement_count != end.placement_count
        || begin.target_count != end.target_count
    {
        return Err(invalid());
    }
    let mut candidate = ClientContentCandidate {
        transaction,
        begin: begin.clone(),
        surfaces: Vec::new(),
        targets: Vec::new(),
    };
    let mut placements = 0usize;
    for (ordinal, record) in records[1..records.len() - 1].iter().enumerate() {
        let ShellContentRecord::CandidateChunk(chunk) = record else {
            return Err(invalid());
        };
        if chunk.grant != begin.grant
            || chunk.candidate_generation != begin.candidate_generation
            || chunk.chunk_ordinal as usize != ordinal
        {
            return Err(invalid());
        }
        placements += chunk.placements.len();
        if placements > begin.placement_count as usize
            || candidate.surfaces.len() + chunk.surfaces.len() > begin.surface_count as usize
            || candidate.targets.len() + chunk.targets.len() > begin.target_count as usize
        {
            return Err(invalid());
        }
        candidate.surfaces.extend_from_slice(&chunk.surfaces);
        candidate.targets.extend_from_slice(&chunk.targets);
    }
    if placements != begin.placement_count as usize
        || candidate.surfaces.len() != begin.surface_count as usize
        || candidate.targets.len() != begin.target_count as usize
    {
        return Err(invalid());
    }
    Ok(candidate)
}

#[cfg(test)]
#[path = "../tests/support/candidate.rs"]
mod tests;
