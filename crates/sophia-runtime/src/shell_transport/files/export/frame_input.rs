//! Move decoded scene rows once, restoring the exact queued input on deferral.
use super::*;
use crate::{ContentCandidateError, ContentFrameAdmission, ContentFrameCommit};

impl ShellFiles {
    pub(in crate::shell_transport) fn with_frame_commit(
        &mut self,
        admit: impl FnOnce(
            &mut Option<ContentFrameCommit>,
        ) -> Result<ContentFrameAdmission, ContentCandidateError>,
    ) -> Result<Option<ContentFrameAdmission>, ContentCandidateError> {
        let Some(at) = self
            .inbound
            .iter()
            .position(|item| matches!(item, Inbound::Frame(_)))
        else {
            return Ok(None);
        };
        let Inbound::Frame(record) = &mut self.inbound[at] else {
            unreachable!("selected SFT input");
        };
        let ShellFrameRecord::Commit(commit) = record.as_mut() else {
            return Ok(None);
        };
        let candidate = &mut commit.candidate;
        let mut input = Some(ContentFrameCommit {
            transaction: commit.transaction,
            scene: sophia_protocol::ContentCandidate {
                grant: candidate.grant,
                candidate_generation: candidate.candidate_generation,
                output: candidate.output,
                facts_generation: candidate.facts_generation,
                pacing_permit: candidate.pacing_permit,
                interaction_generation: candidate.interaction_generation,
                surfaces: std::mem::take(&mut candidate.surfaces),
                placements: std::mem::take(&mut candidate.placements),
                targets: std::mem::take(&mut candidate.targets),
            },
            image: commit
                .image
                .map(|image| crate::owned_ring_protocol::Offer::from(image).request),
            retire: commit.retire.map(|retire| crate::ContentRetireOnPresent {
                retire_transaction: retire.transaction,
                resource: retire.resource,
            }),
        });
        let decision = admit(&mut input);
        match input {
            Some(input) => {
                commit.candidate = input.scene;
                assert!(
                    matches!(decision, Ok(ContentFrameAdmission::Deferred) | Err(_)),
                    "only deferred or failed admission retains input"
                );
            }
            None => {
                assert!(
                    matches!(
                        decision,
                        Ok(ContentFrameAdmission::Accepted { .. }
                            | ContentFrameAdmission::Refused(_))
                    ),
                    "consumed input owns accepted work or a refusal"
                );
                self.inbound.remove(at);
            }
        }
        if self.lean() && matches!(decision, Ok(ContentFrameAdmission::Accepted { .. })) {
            self.lean_frames_in_flight += 1;
        }
        decision.map(Some)
    }
}
