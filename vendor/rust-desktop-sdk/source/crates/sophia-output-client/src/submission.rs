//! One immutable candidate, independent domain IDs, and bounded custody history.
//! The driver queues an operation, calls `issued`, and supplies its matched reply.
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Fid, QidKind};
use sophia_output_protocol::{output_files::*, *};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Custody {
    Admitted,
    Issued,
    Submitted,
    Refused(u32),
    UnknownDisconnected,
    DroppedUnsent,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Busy,
    Invalid,
    Closed,
    Deadline,
    Encode(BinaryCodecError),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    Walk,
    Open,
    Write,
    Submit,
    Wait,
    Clunk,
}

/// A single driver pass and its acknowledged journal progress. A retry may
/// happen on a later pass after its backoff or after consumption/ACK progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Progress {
    pub now: u64,
    pub pass: u64,
    pub consumed: u64,
    pub acked: u64,
}

#[derive(Debug, PartialEq)]
pub enum Operation<'a> {
    Walk,
    Open(Fid),
    Write {
        fid: Fid,
        offset: u64,
        bytes: &'a [u8],
    },
    Submit(&'a [u8; 24]),
    Clunk(Fid),
}

struct Active {
    ticket: u64,
    kind: OutputFileKind,
    domain: Option<u64>,
    bytes: Vec<u8>,
    submit: [u8; 24],
    stage: Stage,
    outstanding: bool,
    fid: Fid,
    iounit: u32,
    written: usize,
    sent: usize,
    deadline: u64,
    assembly_deadline: Option<u64>,
    retry_at: Option<u64>,
    retry_delay: u64,
    retry_progress: Progress,
    replied: bool,
}

pub struct Submission {
    epoch: u64,
    next_ticket: Option<u64>,
    next_domain: Option<u64>,
    assembly_timeout: u32,
    submitted_sequence: u64,
    history: [Option<(u64, Custody)>; 64],
    active: Option<Active>,
    closed: bool,
}

impl Submission {
    pub fn new(epoch: u64, limits: OutputFileLimits) -> Result<Self, Error> {
        if epoch == 0 || limits.validate().is_err() {
            return Err(Error::Invalid);
        }
        Ok(Self {
            epoch,
            next_ticket: Some(1),
            next_domain: Some(1),
            assembly_timeout: limits.assembly_timeout_millis,
            submitted_sequence: 0,
            history: [None; 64],
            active: None,
            closed: false,
        })
    }
    pub fn custody(&self, ticket: u64) -> Option<Custody> {
        self.history[ticket as usize % 64]
            .filter(|(id, _)| *id == ticket)
            .map(|(_, state)| state)
    }
    pub fn next_transaction(&mut self) -> Result<u64, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        let id = self.next_domain.ok_or(Error::Invalid)?;
        self.next_domain = id.checked_add(1);
        Ok(id)
    }
    pub fn idle(&self) -> bool {
        self.active.is_none()
    }
    pub fn submitted_sequence(&self) -> u64 {
        self.submitted_sequence
    }
    pub fn waiting_for_ack(&self, acked: u64) -> bool {
        self.active.as_ref().is_some_and(|a| a.stage == Stage::Walk)
            && acked < self.submitted_sequence
    }
    pub fn deadline(&self) -> Option<u64> {
        self.active
            .as_ref()
            .map(|a| a.deadline.min(a.assembly_deadline.unwrap_or(u64::MAX)))
    }
    pub fn retry_at(&self) -> Option<u64> {
        self.active.as_ref().and_then(|a| a.retry_at)
    }

    fn available(&self, now: u64, deadline: u64) -> Result<u64, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.active.is_some() {
            return Err(Error::Busy);
        }
        if deadline <= now {
            return Err(Error::Deadline);
        }
        self.next_ticket.ok_or(Error::Invalid)
    }
    pub fn admit(
        &mut self,
        transaction: TransactionId,
        proposal: &OutputV1Proposal,
        capabilities: u64,
        now: u64,
        deadline: u64,
    ) -> Result<u64, Error> {
        let ticket = self.available(now, deadline)?;
        if capabilities & SOPHIA_OUTPUT_CAPABILITY_CONFIGURE == 0
            || proposal.connection_epoch != self.epoch
        {
            return Err(Error::Invalid);
        }
        // Domain IDs may arrive out of order. The server owns replay history;
        // semantic rejection still spends the domain ID there.
        let body = encode_output_file_proposal(transaction, proposal).map_err(Error::Encode)?;
        let bytes = encode_output_file_record(
            OutputFileHeader {
                kind: OutputFileKind::Proposal,
                connection_epoch: self.epoch,
                submission_id: ticket,
                sequence: 0,
            },
            &body,
        )
        .map_err(Error::Encode)?;
        self.install(
            ticket,
            OutputFileKind::Proposal,
            Some(transaction.raw()),
            bytes,
            deadline,
        );
        Ok(ticket)
    }
    pub fn negotiate(
        &mut self,
        offer: OutputV1ClientHello,
        now: u64,
        deadline: u64,
    ) -> Result<u64, Error> {
        let ticket = self.available(now, deadline)?;
        if ticket != 1 {
            return Err(Error::Invalid);
        }
        if offer.capabilities & SOPHIA_OUTPUT_CAPABILITY_OBSERVE == 0 {
            return Err(Error::Invalid);
        }
        let bytes = encode_output_file_record(
            OutputFileHeader {
                kind: OutputFileKind::Negotiate,
                connection_epoch: self.epoch,
                submission_id: ticket,
                sequence: 0,
            },
            &encode_output_file_negotiate(offer),
        )
        .map_err(Error::Encode)?;
        self.install(ticket, OutputFileKind::Negotiate, None, bytes, deadline);
        Ok(ticket)
    }
    fn install(
        &mut self,
        ticket: u64,
        kind: OutputFileKind,
        domain: Option<u64>,
        bytes: Vec<u8>,
        deadline: u64,
    ) {
        let mut submit = [0; 24];
        submit[..8].copy_from_slice(&self.epoch.to_le_bytes());
        submit[8..16].copy_from_slice(&ticket.to_le_bytes());
        submit[16..20].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        self.next_ticket = ticket.checked_add(1);
        self.set(ticket, Custody::Admitted);
        self.active = Some(Active {
            ticket,
            kind,
            domain,
            bytes,
            submit,
            stage: Stage::Walk,
            outstanding: false,
            fid: Fid::NOFID,
            iounit: 0,
            written: 0,
            sent: 0,
            deadline,
            assembly_deadline: None,
            retry_at: None,
            retry_delay: 0,
            retry_progress: Progress {
                now: 0,
                pass: 0,
                consumed: 0,
                acked: 0,
            },
            replied: false,
        });
    }

    /// Describes the next request without changing custody. `msize` is the
    /// negotiated 9P size. The driver must terminate on a deadline error.
    pub fn operation(&self, p: Progress, msize: u32) -> Result<Option<Operation<'_>>, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        let Some(a) = &self.active else {
            return Ok(None);
        };
        if p.now >= self.deadline().unwrap() {
            return Err(Error::Deadline);
        }
        if a.outstanding {
            return Ok(None);
        }
        Ok(match a.stage {
            Stage::Walk if p.acked < self.submitted_sequence => None,
            Stage::Walk => Some(Operation::Walk),
            Stage::Open => Some(Operation::Open(a.fid)),
            Stage::Write => {
                let mut maximum = msize
                    .checked_sub(23)
                    .filter(|n| *n != 0)
                    .ok_or(Error::Invalid)?;
                if a.iounit != 0 {
                    maximum = maximum.min(a.iounit);
                }
                let end = (a.written + maximum as usize).min(a.bytes.len());
                Some(Operation::Write {
                    fid: a.fid,
                    offset: a.written as u64,
                    bytes: &a.bytes[a.written..end],
                })
            }
            Stage::Submit
                if a.retry_at.is_some_and(|at| {
                    p.pass <= a.retry_progress.pass
                        || (p.now < at
                            && p.consumed == a.retry_progress.consumed
                            && p.acked == a.retry_progress.acked)
                }) =>
            {
                None
            }
            Stage::Submit => Some(Operation::Submit(&a.submit)),
            Stage::Wait => None,
            Stage::Clunk => Some(Operation::Clunk(a.fid)),
        })
    }

    /// Call only after the generic pipeline accepted the request. `walk_fid`
    /// is its newly reserved fid for Walk and NOFID for other operations.
    pub fn issued(&mut self, p: Progress, msize: u32, walk_fid: Fid) -> Result<(), Error> {
        if self.active.as_ref().is_some_and(|a| a.stage != Stage::Walk) && walk_fid != Fid::NOFID {
            return Err(Error::Invalid);
        }
        let sent = match self.operation(p, msize)?.ok_or(Error::Busy)? {
            Operation::Walk if walk_fid == Fid::NOFID => return Err(Error::Invalid),
            Operation::Walk => 0,
            Operation::Write { bytes, .. } => bytes.len(),
            _ if walk_fid != Fid::NOFID => return Err(Error::Invalid),
            _ => 0,
        };
        let a = self.active.as_mut().unwrap();
        a.outstanding = true;
        a.sent = sent;
        match a.stage {
            Stage::Walk => a.fid = walk_fid,
            Stage::Write => {
                a.assembly_deadline
                    .get_or_insert(p.now.saturating_add(u64::from(self.assembly_timeout)));
            }
            Stage::Submit => {
                a.retry_at = None;
                let ticket = a.ticket;
                self.set(ticket, Custody::Issued);
            }
            _ => (),
        }
        Ok(())
    }

    /// Journal validation calls this before any submit reply in the same
    /// dispatch batch is interpreted. Submitted is custody, not semantic commit.
    pub fn submitted(&mut self, sequence: u64, value: OutputFileSubmitted) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        let Some(a) = &self.active else {
            return Err(Error::Invalid);
        };
        if sequence <= self.submitted_sequence
            || value.submission_id != a.ticket
            || value.candidate_kind != a.kind
            || self.custody(a.ticket) != Some(Custody::Issued)
        {
            return Err(Error::Invalid);
        }
        let ticket = a.ticket;
        let domain = a.domain;
        self.set(ticket, Custody::Submitted);
        self.submitted_sequence = sequence;
        if let Some(domain) = domain
            && self.next_domain.is_some_and(|next| next <= domain)
        {
            self.next_domain = domain.checked_add(1);
        }
        let a = self.active.as_mut().unwrap();
        if a.replied {
            a.stage = Stage::Clunk;
        }
        Ok(())
    }

    /// Supplies the reply to the one outstanding lane request. Transport
    /// errors are terminal to the session; the driver calls `disconnect`.
    pub fn reply(&mut self, reply: Reply, p: Progress) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        let a = self.active.as_mut().ok_or(Error::Invalid)?;
        if !a.outstanding {
            return Err(Error::Invalid);
        }
        a.outstanding = false;
        let ticket = a.ticket;
        let custody = self.history[ticket as usize % 64].unwrap().1;
        if let Reply::Error(errno) = reply {
            if a.stage != Stage::Submit {
                return Err(Error::Invalid);
            }
            // Output has no WM-style EALREADY success exception. A refusal
            // after observed custody is contradictory, but cannot erase it.
            if custody == Custody::Submitted || errno.0 == 116 {
                return Err(Error::Invalid);
            }
            if errno.0 == 11 {
                a.retry_delay = if a.retry_delay == 0 {
                    4
                } else {
                    (a.retry_delay * 2).min(256)
                };
                a.retry_at = Some(p.now.saturating_add(a.retry_delay));
                a.retry_progress = p;
                self.set(ticket, Custody::Admitted);
                return Ok(());
            }
            let negotiation = a.kind == OutputFileKind::Negotiate;
            a.stage = Stage::Clunk;
            self.set(ticket, Custody::Refused(errno.0));
            return if negotiation {
                Err(Error::Invalid)
            } else {
                Ok(())
            };
        }
        match (a.stage, reply) {
            (Stage::Walk, Reply::Walk(qids))
                if qids.len() == 1 && qids[0].kind == QidKind::File =>
            {
                a.stage = Stage::Open
            }
            (Stage::Open, Reply::Lopen { qid, iounit }) if qid.kind == QidKind::File => {
                a.iounit = iounit;
                a.stage = Stage::Write;
            }
            (Stage::Write, Reply::Write(n)) if n != 0 && n as usize <= a.sent => {
                a.written += n as usize;
                if a.written == a.bytes.len() {
                    a.stage = Stage::Submit;
                }
            }
            (Stage::Submit, Reply::Write(24)) => {
                a.replied = true;
                a.stage = if custody == Custody::Submitted {
                    Stage::Clunk
                } else {
                    Stage::Wait
                };
            }
            (Stage::Clunk, Reply::Clunk) => self.active = None,
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }

    pub fn disconnect(&mut self) {
        if let Some(a) = self.active.take() {
            let state = match self.custody(a.ticket) {
                Some(Custody::Issued) => Custody::UnknownDisconnected,
                _ => Custody::DroppedUnsent,
            };
            self.set(a.ticket, state);
        }
        self.closed = true;
    }
    fn set(&mut self, ticket: u64, value: Custody) {
        let slot = &mut self.history[ticket as usize % 64];
        if matches!(slot, Some((id, state)) if *id == ticket && !matches!(state, Custody::Admitted | Custody::Issued))
        {
            return;
        }
        *slot = Some((ticket, value));
    }
}
