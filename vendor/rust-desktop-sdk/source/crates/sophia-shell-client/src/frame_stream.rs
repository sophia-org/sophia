//! One-credit SFT lifecycle, committed only after the existing queue accepts work.
use crate::{Admission, ShellClientError, ShellConnection};
use sophia_shell_protocol::TransactionId;
use sophia_shell_protocol::shell_files::{
    SHELL_FRAME_REVISION, ShellFileHeader, ShellFileKind, ShellFrameRecord, ShellFrameStreamKey,
    shell_frame_record_bytes,
};

/// Lifecycle of one output binding. Unknown custody cannot be reopened.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FrameStreamState {
    #[default]
    Closed,
    Opening,
    Open,
    Closing,
    Unknown,
}

#[derive(Debug)]
struct Pending {
    transaction: TransactionId,
    generation: u64,
    prepared: bool,
}

/// Owns frame credit and exact completion correlation, without rendering or I/O.
/// Route every SFT event here in journal order and call `disconnect` on transport
/// loss/revocation. This tracks frame work, not image-slot or resource custody;
/// keep those owners alive independently. Do not recreate it to escape Unknown.
#[derive(Debug, Default)]
pub struct FrameStream {
    state: FrameStreamState,
    key: Option<ShellFrameStreamKey>,
    opening: Option<TransactionId>,
    closing: Option<TransactionId>,
    last_stream: u64,
    last_candidate: u64,
    credit: bool,
    closed_notice: bool,
    pending: Option<Pending>,
}

impl FrameStream {
    /// An empty stream owner, before any authority or credit is granted.
    pub fn new() -> Self {
        Self::default()
    }
    /// Current frame-control phase; this does not report image-source custody.
    pub fn state(&self) -> FrameStreamState {
        self.state
    }
    /// Proposed or accepted binding, including while its close notice drains.
    pub fn key(&self) -> Option<ShellFrameStreamKey> {
        self.key
    }
    /// Available local credit is provisional: the owner can still refuse expiry.
    pub fn has_credit(&self) -> bool {
        self.state == FrameStreamState::Open && self.credit
    }
    /// No outstanding frame or stream notice; source/resource owners are separate.
    pub fn frame_work_drained(&self) -> bool {
        self.state == FrameStreamState::Closed
    }

    /// Queue Open, or reopen after both Closed and the old terminal have drained.
    /// A reused owner stays within one grant and preserves generation high waters.
    /// # Errors
    /// Refuses live/unknown work, a replaced grant, nonincreasing accepted stream
    /// ids, invalid wire values or queue capacity. Refusal leaves this owner intact.
    pub fn open(
        &mut self,
        connection: &mut ShellConnection,
        transaction: TransactionId,
        key: ShellFrameStreamKey,
    ) -> Result<Admission, ShellClientError> {
        if self.state != FrameStreamState::Closed
            || key.stream_id <= self.last_stream
            || self.key.is_some_and(|old| old.grant != key.grant)
        {
            return Err(ShellClientError::InvalidArgument(
                "frame stream cannot reopen",
            ));
        }
        let admission = connection.enqueue_frame(&ShellFrameRecord::Open { transaction, key })?;
        self.state = FrameStreamState::Opening;
        self.key = Some(key);
        self.opening = Some(transaction);
        self.closing = None;
        self.closed_notice = false;
        self.credit = false;
        Ok(admission)
    }

    /// Queue one whole Commit, reserving the sole credit only on queue acceptance.
    /// Image-slot ownership must be joined by the producer before this call;
    /// neither a custody receipt nor Prepared returns this credit.
    /// # Errors
    /// Refuses wrong binding/direction, nonincreasing generation, unavailable
    /// credit, invalid wire shape or queue capacity without changing stream state.
    pub fn submit(
        &mut self,
        connection: &mut ShellConnection,
        record: &ShellFrameRecord,
    ) -> Result<Admission, ShellClientError> {
        self.check_submit(record)?;
        let admission = connection.enqueue_frame(record)?;
        self.submitted(record);
        Ok(admission)
    }

    pub(crate) fn check_submit(&self, record: &ShellFrameRecord) -> Result<(), ShellClientError> {
        if self.state != FrameStreamState::Open {
            return Err(ShellClientError::InvalidArgument(
                "frame stream is not open",
            ));
        }
        let ShellFrameRecord::Commit(value) = record else {
            return Err(ShellClientError::WrongDirection);
        };
        let key = self.key.ok_or(ShellClientError::InvalidArgument(
            "frame stream has no binding",
        ))?;
        let candidate = &value.candidate;
        if candidate.grant != key.grant
            || candidate.output != key.output
            || candidate.facts_generation != key.facts_generation
            || candidate.pacing_permit != key.stream_id
            || candidate.surfaces.len() != 1
            || candidate.surfaces[0].allocation != key.allocation
            || candidate.candidate_generation <= self.last_candidate
        {
            return Err(ShellClientError::InvalidArgument(
                "frame does not match stream binding",
            ));
        }
        if !self.has_credit() || self.pending.is_some() {
            return Err(ShellClientError::QueueSaturated);
        }
        Ok(())
    }

    pub(crate) fn submitted(&mut self, record: &ShellFrameRecord) {
        let ShellFrameRecord::Commit(value) = record else {
            unreachable!("validated frame commit")
        };
        let candidate = &value.candidate;
        self.last_candidate = candidate.candidate_generation;
        self.credit = false;
        self.pending = Some(Pending {
            transaction: value.transaction,
            generation: candidate.candidate_generation,
            prepared: false,
        });
    }

    /// Stop admissions immediately after Close is queued. A refused Close may be
    /// retried; it never reopens admissions or cancels an already accepted frame.
    /// # Errors
    /// Refuses Opening/Closed/Unknown, a pending close, the opening transaction
    /// (automatic expiry must stay distinguishable), invalid input or capacity.
    pub fn close(
        &mut self,
        connection: &mut ShellConnection,
        transaction: TransactionId,
    ) -> Result<Admission, ShellClientError> {
        if !matches!(
            self.state,
            FrameStreamState::Open | FrameStreamState::Closing
        ) || self.closing.is_some()
            || self.closed_notice
            || Some(transaction) == self.opening
        {
            return Err(ShellClientError::InvalidArgument(
                "frame stream cannot close",
            ));
        }
        let key = self.key.ok_or(ShellClientError::InvalidArgument(
            "frame stream has no binding",
        ))?;
        let admission = connection.enqueue_frame(&ShellFrameRecord::Close { transaction, key })?;
        self.state = FrameStreamState::Closing;
        self.credit = false;
        self.closing = Some(transaction);
        Ok(admission)
    }

    /// Lose authority without guessing whether a source or frame finished.
    pub fn disconnect(&mut self) {
        self.state = FrameStreamState::Unknown;
        self.credit = false;
    }

    /// Apply a validated journal event in order, exactly once. Deadlines are
    /// opaque owner time: this never renews credit using the client's clock.
    /// # Errors
    /// Wrong identities, duplicate/out-of-order events and malformed values
    /// poison this owner; it admits nothing further and retains outstanding work.
    pub fn observe(&mut self, record: &ShellFrameRecord) -> Result<(), ShellClientError> {
        let valid = self.key.is_some_and(|key| {
            shell_frame_record_bytes(
                ShellFileHeader {
                    kind: record.kind(),
                    connection_epoch: key.grant.connection_epoch,
                    submission_id: 0,
                    sequence: 1,
                },
                record,
                SHELL_FRAME_REVISION,
            )
            .is_ok()
        });
        if !valid || self.state == FrameStreamState::Unknown || !self.apply(record) {
            self.disconnect();
            return Err(ShellClientError::Protocol(
                "invalid frame stream observation",
            ));
        }
        Ok(())
    }

    fn apply(&mut self, record: &ShellFrameRecord) -> bool {
        let Some(key) = self.key else {
            return false;
        };
        match record {
            ShellFrameRecord::Opened {
                transaction,
                key: observed,
                ..
            } if self.state == FrameStreamState::Opening
                && *observed == key
                && Some(*transaction) == self.opening =>
            {
                self.last_stream = key.stream_id;
                self.state = FrameStreamState::Open;
                self.credit = true;
            }
            ShellFrameRecord::Closed {
                transaction,
                key: observed,
                ..
            } if matches!(
                self.state,
                FrameStreamState::Open | FrameStreamState::Closing
            ) && !self.closed_notice
                && *observed == key
                && (Some(*transaction) == self.opening || Some(*transaction) == self.closing) =>
            {
                self.closed_notice = true;
                self.credit = false;
                if Some(*transaction) == self.closing {
                    self.closing = None;
                }
                self.state = FrameStreamState::Closing;
                self.finish_drain();
            }
            ShellFrameRecord::Refused {
                transaction,
                grant,
                output,
                stream_id,
                candidate_generation,
                request_kind,
                ..
            } if *grant == key.grant && *output == key.output && *stream_id == key.stream_id => {
                match request_kind {
                    ShellFileKind::FrameStreamOpen
                        if self.state == FrameStreamState::Opening
                            && Some(*transaction) == self.opening =>
                    {
                        self.state = FrameStreamState::Closed;
                    }
                    ShellFileKind::FrameStreamClose
                        if self.state == FrameStreamState::Closing
                            && Some(*transaction) == self.closing =>
                    {
                        self.closing = None;
                        self.finish_drain();
                    }
                    ShellFileKind::FrameCommit
                        if self.pending.as_ref().is_some_and(|pending| {
                            pending.transaction == *transaction
                                && pending.generation == *candidate_generation
                                && !pending.prepared
                        }) =>
                    {
                        self.pending = None;
                        // The refused request spent no host credit; release only our local reservation.
                        self.credit = self.state == FrameStreamState::Open;
                        self.finish_drain();
                    }
                    _ => return false,
                }
            }
            ShellFrameRecord::Outcome {
                transaction,
                outcome,
                stream_id,
                allocation,
                facts_generation,
                credit_expires_msec,
            } if outcome.grant == key.grant
                && outcome.output == key.output
                && *stream_id == key.stream_id
                && *allocation == key.allocation
                && *facts_generation == key.facts_generation =>
            {
                let Some(pending) = &mut self.pending else {
                    return false;
                };
                if pending.transaction != *transaction
                    || pending.generation != outcome.candidate_generation
                {
                    return false;
                }
                if outcome.kind == 1 {
                    if pending.prepared {
                        return false;
                    }
                    pending.prepared = true;
                } else {
                    if self.closed_notice && *credit_expires_msec != 0 {
                        return false;
                    }
                    self.pending = None;
                    self.credit = self.state == FrameStreamState::Open && *credit_expires_msec != 0;
                    if !self.credit {
                        self.state = FrameStreamState::Closing;
                    }
                    self.finish_drain();
                }
            }
            _ => return false,
        }
        true
    }
    fn finish_drain(&mut self) {
        if self.closed_notice && self.pending.is_none() && self.closing.is_none() {
            self.state = FrameStreamState::Closed;
        }
    }
}
