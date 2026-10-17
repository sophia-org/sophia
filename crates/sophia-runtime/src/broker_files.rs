//! Connection custody for the developing broker export. This is not admission:
//! the service must first prove MetadataBroker protection and exact peer identity.
use sophia_9p::{
    Errno,
    export::ReadOutcome,
    journal::{Journal, JournalBounds},
};
use sophia_protocol::broker_files::*;
use std::time::{Duration, Instant};

mod filesystem;
mod pending;
mod service;
pub use filesystem::{BrokerFileEffects, BrokerFileExport, BrokerFileHandle, BrokerFileNode};
use pending::Pending;
pub use service::{BrokerFileListener, BrokerFileService, SOPHIA_BROKER_9P_SOCKET_ENV};

const EALREADY: Errno = Errno(114);
struct Accepted {
    bytes: Vec<u8>,
    id: u64,
    receipt: u64,
}
/// One epoch's journal and candidate retention. The owning service invalidates
/// all handles on close and calls expire even when no client request arrives.
pub struct BrokerFileCustody {
    epoch: u64,
    limits: BrokerFileLimits,
    journal: Journal,
    accepted: Option<Accepted>,
    watermark: u64,
    negotiated: bool,
    refusal: Option<(u64, Instant)>,
    ack_due: Option<Instant>,
    closed: bool,
    pending: Option<Pending>,
    last_request: u64,
}
impl BrokerFileCustody {
    pub fn new(epoch: u64, limits: BrokerFileLimits) -> Result<Self, Errno> {
        Self::new_after(epoch, limits, 0)
    }
    /// The Session owner carries this watermark across connection replacements.
    pub fn new_after(
        epoch: u64,
        limits: BrokerFileLimits,
        last_request: u64,
    ) -> Result<Self, Errno> {
        limits.validate().map_err(|_| Errno::EINVAL)?;
        if epoch == 0 {
            return Err(Errno::EINVAL);
        }
        Ok(Self {
            epoch,
            limits,
            journal: Journal::new(epoch),
            accepted: None,
            watermark: 0,
            negotiated: false,
            refusal: None,
            ack_due: None,
            closed: false,
            pending: None,
            last_request,
        })
    }
    fn bounds(&self) -> JournalBounds {
        JournalBounds {
            records: self.limits.journal_records as usize,
            bytes: self.limits.journal_bytes as usize,
        }
    }
    fn due(&self, now: Instant) -> Result<Instant, Errno> {
        now.checked_add(Duration::from_millis(u64::from(
            self.limits.ack_progress_timeout_ms,
        )))
        .ok_or(Errno::ENOSPC)
    }
    pub fn close(&mut self) {
        self.closed = true;
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    pub fn negotiated(&self) -> bool {
        self.negotiated && !self.closed
    }
    pub fn expire(&mut self, now: Instant) {
        if self.ack_due.is_some_and(|d| now >= d)
            || self.refusal.is_some_and(|(_, d)| now >= d)
            || self.pending.as_ref().is_some_and(|p| now >= p.due)
        {
            self.close();
        }
    }
    fn live(&mut self, now: Instant) -> Result<(), Errno> {
        self.expire(now);
        if self.closed {
            Err(Errno::ESTALE)
        } else {
            Ok(())
        }
    }
    pub fn retained(&self) -> Option<&[u8]> {
        self.accepted.as_ref().map(|a| a.bytes.as_slice())
    }
    pub fn read(&mut self, offset: u64, count: u32, now: Instant) -> Result<ReadOutcome, Errno> {
        self.live(now)?;
        self.journal.read(offset, count)
    }
    /// The file layer supplies the complete staging bytes. Exact retries before
    /// receipt ack publish nothing; later use of the accepted ID is EALREADY.
    pub fn negotiate(
        &mut self,
        control: &[u8],
        candidate: &[u8],
        now: Instant,
    ) -> Result<(), Errno> {
        self.live(now)?;
        let c = decode_broker_file_submit(control).map_err(|_| Errno::EINVAL)?;
        if c.epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if c.length as usize != candidate.len() {
            return Err(Errno::EINVAL);
        }
        let request = decode_broker_file_negotiate(candidate).map_err(|_| Errno::EINVAL)?;
        if request.epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if request.submission != c.submission {
            return Err(Errno::EINVAL);
        }
        if let Some(accepted) = &self.accepted {
            if c.submission == accepted.id {
                return if candidate == accepted.bytes {
                    Ok(())
                } else {
                    Err(Errno::EINVAL)
                };
            }
            if c.submission < accepted.id {
                return Err(EALREADY);
            }
            return Err(Errno::EAGAIN);
        }
        if c.submission <= self.watermark {
            return Err(EALREADY);
        }
        if self.refusal.is_some() {
            return Err(Errno::EACCES);
        }
        if self.negotiated {
            return Err(EALREADY);
        }
        let selected =
            request.minimum <= 2 && request.maximum >= 2 && request.minimum <= request.maximum;
        let first = self.journal.next_sequence();
        let last = first.checked_add(1).ok_or(Errno::ENOSPC)?;
        let receipt = encode_broker_file_submitted(self.epoch, first, c.submission, 256)
            .map_err(|_| Errno::EINVAL)?;
        let outcome = if selected {
            self.limits
                .negotiated(self.epoch, last)
                .map_err(|_| Errno::EINVAL)?
                .to_vec()
        } else {
            encode_broker_file_refused(self.epoch, last)
                .map_err(|_| Errno::EINVAL)?
                .to_vec()
        };
        let due = self.due(now)?;
        let accepted = Accepted {
            bytes: candidate.to_vec(),
            id: c.submission,
            receipt: first,
        };
        let bounds = self.bounds();
        self.journal
            .prepare_batch(vec![receipt.to_vec(), outcome], bounds)?
            .commit();
        self.accepted = Some(accepted);
        self.watermark = c.submission;
        self.negotiated = selected;
        self.ack_due = Some(due);
        if !selected {
            self.refusal = Some((last, due));
        }
        Ok(())
    }
    pub fn last_request(&self) -> u64 {
        self.last_request
    }
    /// Reserve the request and its future receipt before publishing. No identity
    /// is spent on capacity failure. Only one request may await a response.
    pub fn publish(
        &mut self,
        id: u64,
        request: BrokerFileRequest<'_>,
        now: Instant,
    ) -> Result<(), Errno> {
        self.live(now)?;
        if !self.negotiated || self.refusal.is_some() {
            return Err(Errno::EACCES);
        }
        if id == 0 || id <= self.last_request {
            return Err(EALREADY);
        }
        if self.pending.is_some() || self.accepted.is_some() {
            return Err(Errno::EAGAIN);
        }
        let seq = self.journal.next_sequence();
        seq.checked_add(2).ok_or(Errno::ENOSPC)?;
        self.journal.size().checked_add(248).ok_or(Errno::ENOSPC)?;
        let bytes = request
            .encode(self.epoch, seq, id)
            .map_err(|_| Errno::EINVAL)?;
        if let BrokerFileRequest::CandidateReduced {
            label: Some(text), ..
        } = request
            && text.len() > usize::from(self.limits.max_label_bytes)
        {
            return Err(Errno::EINVAL);
        }
        let response_due = now
            .checked_add(Duration::from_millis(u64::from(
                self.limits.response_timeout_ms,
            )))
            .ok_or(Errno::ENOSPC)?;
        let ack_due = self.due(now)?;
        let pending = Pending::new(id, request, response_due);
        let bounds = self.bounds();
        self.journal
            .prepare(
                bytes.to_vec(),
                JournalBounds {
                    records: bounds.records - 1,
                    bytes: bounds.bytes - 48,
                },
            )?
            .commit();
        self.pending = Some(pending);
        self.last_request = id;
        self.ack_due.get_or_insert(ack_due);
        Ok(())
    }
    /// prepare validates current owner state and reserves every effect without
    /// applying any. commit must be infallible. A failed prepare publishes no
    /// receipt, applies no row and takes no candidate custody.
    pub fn respond<T>(
        &mut self,
        control: &[u8],
        candidate: &[u8],
        now: Instant,
        prepare: impl FnOnce(&BrokerFileResponseSet<'_>) -> Result<T, Errno>,
        commit: impl FnOnce(T),
    ) -> Result<(), Errno> {
        self.live(now)?;
        let control = decode_broker_file_submit(control).map_err(|_| Errno::EINVAL)?;
        if control.epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if candidate.len() != control.length as usize || candidate.len() != 448 {
            return Err(Errno::EINVAL);
        }
        if let Some(accepted) = &self.accepted {
            if control.submission == accepted.id {
                return if candidate == accepted.bytes {
                    Ok(())
                } else {
                    Err(Errno::EINVAL)
                };
            }
            if control.submission < accepted.id {
                return Err(EALREADY);
            }
            return Err(Errno::EAGAIN);
        }
        if control.submission <= self.watermark {
            return Err(EALREADY);
        }
        if !self.negotiated || self.refusal.is_some() {
            return Err(Errno::EACCES);
        }
        let pending = self.pending.as_ref().ok_or(Errno::EINVAL)?;
        let set = decode_broker_file_response(candidate, pending.id, pending.view())
            .map_err(|_| Errno::EINVAL)?;
        if set.epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if set.submission != control.submission {
            return Err(Errno::EINVAL);
        }
        if set.rows().any(|r| {
            r.label
                .is_some_and(|l| l.len() > usize::from(self.limits.max_label_bytes))
        }) {
            return Err(Errno::EINVAL);
        }
        let sequence = self.journal.next_sequence();
        let receipt = encode_broker_file_submitted(self.epoch, sequence, control.submission, 257)
            .map_err(|_| Errno::EINVAL)?;
        let due = self.due(now)?;
        let accepted = Accepted {
            bytes: candidate.to_vec(),
            id: control.submission,
            receipt: sequence,
        };
        let bounds = self.bounds();
        let journal = self.journal.prepare(receipt.to_vec(), bounds)?;
        let effects = prepare(&set)?;
        commit(effects);
        journal.commit();
        self.accepted = Some(accepted);
        self.watermark = control.submission;
        self.pending = None;
        self.ack_due.get_or_insert(due);
        Ok(())
    }
    pub fn acknowledge(&mut self, bytes: &[u8], now: Instant) -> Result<(), Errno> {
        self.live(now)?;
        let ack = decode_broker_file_ack(bytes).map_err(|_| Errno::EINVAL)?;
        let due = self.due(now)?;
        if self.journal.ack(ack.epoch, ack.sequence)? {
            if self
                .accepted
                .as_ref()
                .is_some_and(|a| ack.sequence >= a.receipt)
            {
                self.accepted = None;
            }
            self.ack_due = (self.journal.position().records != 0).then_some(due);
        }
        if self.refusal.is_some_and(|(seq, _)| ack.sequence >= seq) {
            self.close();
        }
        Ok(())
    }
}
