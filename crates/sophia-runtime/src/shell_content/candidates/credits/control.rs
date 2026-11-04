use super::*;
use sophia_protocol::shell_files::ShellFileKind;

impl ContentCandidateStore {
    /// Admit a native stream Open with one credit and a reserved refusal path.
    ///
    /// `response_room` includes the connection's stores, FIFO and byte limits.
    /// `consume` must take exactly the peeked input without I/O or failure.
    /// No input is consumed without room for a reply; successful Open needs
    /// both its reply and its lifetime close reservation.
    /// No current owner context is a typed Stale refusal after reserving its reply.
    /// Owner-clock expiry still progresses while input is deferred.
    ///
    /// # Errors
    /// Invalid grant/profile, malformed identity or clock regression fail before
    /// input consumption. Valid but refused requests receive a typed response.
    pub fn admit_frame_open(
        &mut self,
        transaction: TransactionId,
        key: FrameStreamKey,
        context: Option<ContentCandidateContext<'_>>,
        now: u64,
        response_room: usize,
        consume: impl FnOnce(),
    ) -> Result<ContentFrameRequestAdmission, ContentCandidateError> {
        self.check_grant(key.grant)?;
        if !key.valid() {
            return Err(ContentCandidateError::Malformed);
        }
        let room = self.frame_admission_room(transaction, key.grant, now, response_room)?;
        if room == 0 {
            return Ok(ContentFrameRequestAdmission::Deferred);
        }
        let result = context.map_or(Err(ContentCandidateError::Stale), |context| {
            self.open_frame_stream_with_room(transaction, key, 1, context, now, room)
        });
        consume();
        Ok(self.finish_frame_control(transaction, key, ShellFileKind::FrameStreamOpen, result))
    }

    /// Admit Close with room for a refusal even when expiry won the race.
    ///
    /// A successful close uses the lifetime reservation; it adds no new one.
    /// The input custody requirements are the same as [`Self::admit_frame_open`].
    ///
    /// # Errors
    /// Invalid grant/profile, malformed identity or clock regression fail before
    /// input consumption. A stale or already closed stream is a typed refusal.
    pub fn admit_frame_close(
        &mut self,
        transaction: TransactionId,
        key: FrameStreamKey,
        now: u64,
        response_room: usize,
        consume: impl FnOnce(),
    ) -> Result<ContentFrameRequestAdmission, ContentCandidateError> {
        self.check_grant(key.grant)?;
        if !key.valid() {
            return Err(ContentCandidateError::Malformed);
        }
        let room = self.frame_admission_room(transaction, key.grant, now, response_room)?;
        if room == 0 {
            return Ok(ContentFrameRequestAdmission::Deferred);
        }
        let result = self.close_frame_stream(transaction, key, now);
        consume();
        Ok(self.finish_frame_control(transaction, key, ShellFileKind::FrameStreamClose, result))
    }

    pub(super) fn frame_admission_room(
        &mut self,
        transaction: TransactionId,
        grant: ContentGrant,
        now: u64,
        response_room: usize,
    ) -> Result<usize, ContentCandidateError> {
        self.check_grant(grant)?;
        if self.profile != ContentStoreProfile::Legacy || !transaction.is_valid() {
            return Err(ContentCandidateError::Stale);
        }
        self.time(now)?;
        self.expire(now)?;
        Ok(response_room.min(
            (self.limits.max_control_records as usize).saturating_sub(self.control_occupancy()),
        ))
    }

    fn finish_frame_control(
        &mut self,
        transaction: TransactionId,
        key: FrameStreamKey,
        request_kind: ShellFileKind,
        result: Result<(), ContentCandidateError>,
    ) -> ContentFrameRequestAdmission {
        match result {
            Ok(()) => ContentFrameRequestAdmission::Accepted,
            Err(error) => {
                self.events.push_stream(ContentFrameStreamEvent {
                    transaction,
                    record: ContentFrameStreamRecord::Refused {
                        request_kind,
                        grant: key.grant,
                        output: key.output,
                        stream_id: key.stream_id,
                        candidate_generation: 0,
                        reason: error.reason(),
                    },
                });
                ContentFrameRequestAdmission::Refused(error)
            }
        }
    }
}
