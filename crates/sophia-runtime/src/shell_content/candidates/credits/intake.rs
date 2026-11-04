use super::*;

/// Whether a complete frame input remains queued or has transferred custody.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentFrameBeginAdmission {
    Deferred,
    Accepted,
    /// The caller must discard this input's remaining internal candidate parts.
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
    ) -> Result<ContentFrameBeginAdmission, ContentCandidateError> {
        self.check_grant(begin.grant)?;
        if self.profile != ContentStoreProfile::Legacy || !transaction.is_valid() {
            return Err(ContentCandidateError::Stale);
        }
        self.time(now)?;
        self.expire(now)?;
        let room = response_room.min(
            (self.limits.max_control_records as usize).saturating_sub(self.control_occupancy()),
        );
        if room == 0 {
            return Ok(ContentFrameBeginAdmission::Deferred);
        }
        // No store mutation or I/O can use this reserved refusal slot before
        // the synchronous consume/admit/report operation completes.
        let known_stream = self.streams.entries.contains_key(&begin.output);
        let needed = 2 - usize::from(self.pending.contains_key(&begin.output));
        let invalid = (room < needed).then_some(ContentCandidateError::Budget);
        let output = begin.output;
        let stream_id = begin.pacing_permit;
        let generation = begin.candidate_generation;
        consume();
        let result = if known_stream {
            self.begin_inner(transaction, begin, None, None, invalid, now)
        } else {
            // Never reinterpret a stale FrameCommit as a legacy pacing permit.
            Err(ContentCandidateError::Stale)
        };
        match result {
            Ok(()) => Ok(ContentFrameBeginAdmission::Accepted),
            Err(error) => {
                self.events.push_stream(ContentFrameStreamEvent {
                    transaction,
                    record: ContentFrameStreamRecord::Refused {
                        grant: self.limits.grant,
                        output,
                        stream_id,
                        candidate_generation: generation,
                        reason: error.reason(),
                    },
                });
                Ok(ContentFrameBeginAdmission::Refused(error))
            }
        }
    }
}
