//! Reserve complete SFT intent before taking the decoder's bounded input.
use super::frame::frame_error;
use super::*;
use crate::shell_content::candidates::{FrameAdmissionReservation, FrameCommitState};
use crate::{ContentCandidateError as Error, ContentFrameBeginAdmission as Admission};

impl ContentEpochRegistry {
    pub(crate) fn reserve_frame_commit(
        &mut self,
        worker: &mut crate::IsbValidationWorker,
        ring: Option<&crate::OwnedRing>,
        input: &mut Option<crate::ContentFrameCommit>,
        response_room: usize,
        job_slot_available: bool,
        now: u64,
        deadline: std::time::Instant,
    ) -> Result<(Admission, Option<crate::OwnedCopyImageAdmission>), Error> {
        let frame = input.as_ref().ok_or(Error::Stale)?;
        let begin = frame.begin();
        let epoch = self
            .active
            .iter_mut()
            .find(|epoch| epoch.resources.grant() == begin.grant)
            .ok_or(Error::Stale)?;
        if epoch.resources.is_revoked() {
            return Err(Error::Stale);
        }
        if frame.image.is_some()
            && ring.is_none_or(|ring| {
                ring.grant() != begin.grant || !ring.is_charged_to(&epoch.source_bytes)
            })
        {
            return Err(Error::Stale);
        }
        let key = epoch.candidates.frame_stream_key(begin.output);
        let input_error = key
            .filter(|key| {
                frame.image.is_some_and(|image| {
                    image.allocation != key.allocation
                        || image.output != key.output
                        || image.topology_generation != key.facts_generation
                }) || frame
                    .scene
                    .surfaces
                    .first()
                    .is_some_and(|surface| surface.allocation != key.allocation)
            })
            .map(|_| Error::Stale);
        let shape = epoch.candidates.validate_frame_scene(frame);
        let owner = crate::ContentRetireOwner {
            grant: begin.grant,
            output: begin.output,
            candidate_generation: begin.candidate_generation,
        };
        let mut admission = None;
        let decision = epoch.candidates.admit_frame_begin_with(
            frame.transaction,
            begin,
            now,
            response_room,
            input_error,
            || {
                shape?;
                if frame.image.is_some()
                    && ring.expect("copy source checked").geometry().slot_count() != 2
                {
                    return Err(Error::Malformed);
                }
                epoch.resources.validate_frame_cpu_layers(
                    owner,
                    &frame.scene.placements,
                    frame.image.map(|image| image.resource),
                    frame.retire.map(|retire| retire.resource),
                )?;
                if frame.image.is_some() && (!job_slot_available || response_room < 4) {
                    return Err(Error::Budget);
                }
                let retire = if let Some(retire) = frame.retire {
                    let token = crate::ContentRetireToken::default();
                    epoch.resources.reserve_retire(owner, retire, &token)?;
                    Some((retire, token))
                } else {
                    None
                };
                if let Some(image) = frame.image {
                    let reserved = reserve_copy(
                        &epoch.allocations,
                        &mut epoch.resources,
                        &epoch.source_bytes,
                        worker,
                        ring.expect("copy source checked"),
                        image,
                        now,
                        deadline,
                    )
                    .map_err(frame_error)?;
                    epoch.resources.bind_frame_copy(image.resource, owner);
                    admission = Some(reserved);
                }
                Ok(FrameAdmissionReservation {
                    copy_job: admission.as_ref().map(|copy| copy.worker_job),
                    commit: Some(FrameCommitState { retire }),
                })
            },
            || {},
        )?;
        if decision != Admission::Deferred {
            let frame = input.take().expect("peeked input remains owned");
            if decision == Admission::Accepted {
                epoch.candidates.install_frame_scene(frame.scene);
                if let Some(copy) = &mut admission {
                    copy.metadata.frame = Some(epoch.candidates.frame_copy_binding(owner.output));
                }
            }
        }
        Ok((decision, admission))
    }
}
