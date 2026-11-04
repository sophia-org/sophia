//! Join the existing frame-credit, resource and copy-worker reservations.
use super::*;
use crate::{ContentCandidateError, ContentFrameRequestAdmission, IsbImageAdmissionError};

impl ContentEpochRegistry {
    pub(crate) fn reserve_owned_frame_copy(
        &mut self,
        worker: &mut crate::IsbValidationWorker,
        ring: &crate::OwnedRing,
        frame: crate::OwnedFrameCopyRequest,
        response_room: usize,
        job_slot_available: bool,
        now: u64,
        deadline: std::time::Instant,
        consume: impl FnOnce(),
    ) -> Result<
        (
            ContentFrameRequestAdmission,
            Option<crate::OwnedCopyImageAdmission>,
        ),
        ContentCandidateError,
    > {
        let crate::OwnedFrameCopyRequest {
            transaction,
            begin,
            image: request,
        } = frame;
        if begin.grant != ring.grant() {
            return Err(ContentCandidateError::Stale);
        }
        let epoch = self
            .active
            .iter_mut()
            .find(|epoch| epoch.resources.grant() == begin.grant)
            .ok_or(ContentCandidateError::Stale)?;
        if !ring.is_charged_to(&epoch.source_bytes) || epoch.resources.is_revoked() {
            return Err(ContentCandidateError::Stale);
        }
        let key = epoch.candidates.frame_stream_key(begin.output);
        let input_error = key
            .filter(|key| {
                request.allocation != key.allocation
                    || request.output != key.output
                    || request.topology_generation != key.facts_generation
            })
            .map(|_| ContentCandidateError::Stale);
        let mut admission = None;
        let output = begin.output;
        let owner = crate::ContentRetireOwner {
            grant: begin.grant,
            output,
            candidate_generation: begin.candidate_generation,
        };
        let decision = epoch.candidates.admit_frame_begin_with(
            transaction,
            begin,
            now,
            response_room,
            input_error,
            || {
                // Prepared + terminal, and resource status + eventual release.
                if !job_slot_available || response_room < 4 {
                    return Err(ContentCandidateError::Budget);
                }
                let reserved = reserve_copy(
                    &epoch.allocations,
                    &mut epoch.resources,
                    &epoch.source_bytes,
                    worker,
                    ring,
                    request,
                    now,
                    deadline,
                )
                .map_err(frame_error)?;
                epoch.resources.bind_frame_copy(request.resource, owner);
                let job = reserved.worker_job;
                admission = Some(reserved);
                Ok(
                    crate::shell_content::candidates::FrameAdmissionReservation {
                        copy_job: Some(job),
                        commit: None,
                    },
                )
            },
            consume,
        )?;
        if let Some(admission) = &mut admission {
            admission.metadata.frame = Some(epoch.candidates.frame_copy_binding(output));
        }
        Ok((decision, admission))
    }
}

pub(super) fn frame_error(error: IsbImageAdmissionError) -> ContentCandidateError {
    use crate::{ContentAllocationError as Allocation, IsbSubmitFailure as Worker};
    use ContentCandidateError as Frame;
    match error {
        // The stream identity passed; an invalid resource reference spends it.
        IsbImageAdmissionError::Resource(ContentStoreError::Stale) => Frame::Malformed,
        IsbImageAdmissionError::Resource(error) => error.into(),
        IsbImageAdmissionError::Worker(Worker::Capacity) => Frame::Budget,
        IsbImageAdmissionError::Worker(_) => Frame::RendererFailed,
        IsbImageAdmissionError::Allocation(error) => match error {
            Allocation::Stale => Frame::Stale,
            Allocation::Budget => Frame::Budget,
            Allocation::Malformed => Frame::Malformed,
            Allocation::Timeout => Frame::Timeout,
            Allocation::OutputLost | Allocation::AllocationLost => Frame::AllocationLost,
            Allocation::ClockRegression => Frame::ClockRegression,
        },
    }
}
