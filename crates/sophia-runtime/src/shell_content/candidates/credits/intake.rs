use super::*;

/// Whether an SFT request remains queued or has transferred custody.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentFrameRequestAdmission {
    Deferred,
    Accepted,
    /// Discard any remaining internal candidate parts belonging to this input.
    Refused(ContentCandidateError),
}

impl ContentCandidateStore {
    /// Admit one stream Begin with a reserved, nonfatal refusal path.
    ///
    /// `response_room` is the connection-wide additional response capacity,
    /// including other stores, the FIFO and encoded-byte limits. The store
    /// further restricts it to its own limit. `consume` must infallibly take
    /// exactly the peeked complete input; it is never called when deferred or
    /// when grant validation fails. No I/O may separate the peek and this call.
    /// Continuations of an accepted input use their existing reservation and
    /// must not go through this entry point or reserve another response.
    pub fn admit_frame_begin(
        &mut self,
        transaction: TransactionId,
        begin: ContentCandidateBegin,
        now: u64,
        response_room: usize,
        consume: impl FnOnce(),
    ) -> Result<ContentFrameRequestAdmission, ContentCandidateError> {
        self.admit_frame_begin_with(
            transaction,
            begin,
            now,
            response_room,
            None,
            || Ok(FrameAdmissionReservation::default()),
            consume,
        )
    }

    pub(crate) fn admit_frame_begin_with(
        &mut self,
        transaction: TransactionId,
        begin: ContentCandidateBegin,
        now: u64,
        response_room: usize,
        input_error: Option<ContentCandidateError>,
        reserve_copy: impl FnOnce() -> Result<FrameAdmissionReservation, ContentCandidateError>,
        consume: impl FnOnce(),
    ) -> Result<ContentFrameRequestAdmission, ContentCandidateError> {
        let room = self.frame_admission_room(transaction, begin.grant, now, response_room)?;
        if room == 0 {
            return Ok(ContentFrameRequestAdmission::Deferred);
        }
        // No store mutation or I/O can use this reserved refusal slot before
        // the synchronous consume/admit/report operation completes.
        let known_stream = self.streams.entries.contains_key(&begin.output);
        let needed = 2 - usize::from(self.pending.contains_key(&begin.output));
        let invalid = (room < needed).then_some(ContentCandidateError::Budget);
        let output = begin.output;
        let stream_id = begin.pacing_permit;
        let generation = begin.candidate_generation;
        let result = if let Some(error) = input_error {
            Err(error)
        } else if known_stream {
            self.begin_frame_stream_with(transaction, begin, invalid, now, reserve_copy)
        } else {
            // Never reinterpret a stale FrameCommit as a legacy pacing permit.
            Err(ContentCandidateError::Stale)
        };
        // All accepted work is reserved before removing the peeked input.
        consume();
        match result {
            Ok(()) => Ok(ContentFrameRequestAdmission::Accepted),
            Err(error) => {
                self.events.push_stream(ContentFrameStreamEvent {
                    transaction,
                    record: ContentFrameStreamRecord::Refused {
                        request_kind: sophia_protocol::shell_files::ShellFileKind::FrameCommit,
                        grant: self.limits.grant,
                        output,
                        stream_id,
                        candidate_generation: generation,
                        reason: error.reason(),
                    },
                });
                Ok(ContentFrameRequestAdmission::Refused(error))
            }
        }
    }
}
