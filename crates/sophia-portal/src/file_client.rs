//! Bounded blocking requester for a supplied portal connection. Run this only
//! on a clipboard worker, never on the Session input/render owner. Transport
//! possession is not admission: the server must register its end separately.

use sophia_9p_client::pipeline::{Pipeline, PipelineError, PipelineLimits, Reply};
use sophia_9p_client::records::{Errno, Fid, Tag};
use sophia_protocol::portal_files::*;
use sophia_protocol::{PortalBrokerResponseDecision, PortalGrant, PortalGrantState, PortalRequest};
use std::collections::VecDeque;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum PortalFileClientError {
    Transport(PipelineError),
    Remote(Errno),
    Protocol(&'static str),
}
impl std::fmt::Display for PortalFileClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "portal requester: {self:?}")
    }
}
impl std::error::Error for PortalFileClientError {}
impl From<PipelineError> for PortalFileClientError {
    fn from(error: PipelineError) -> Self {
        Self::Transport(error)
    }
}
type Result<T> = std::result::Result<T, PortalFileClientError>;
use PortalFileClientError::Protocol;

/// One transfer per admitted connection. The overall deadline includes version,
/// negotiation, source collection by the caller, upload and terminal settlement.
pub struct PortalFileClient {
    pipeline: Pipeline,
    root: Fid,
    journal: Fid,
    epoch: u64,
    deadline: Instant,
    offset: u64,
    sequence: u64,
    submission: u64,
    pending: VecDeque<PortalFileEvent>,
    grant: Option<PortalGrant>,
    requested: bool,
    max_payload: u64,
    broker_generation: u64,
}

impl PortalFileClient {
    pub fn over(stream: UnixStream, epoch: u64, budget: Duration) -> Result<Self> {
        let deadline = Instant::now()
            .checked_add(budget)
            .ok_or(Protocol("deadline overflow"))?;
        if epoch == 0 {
            return Err(Protocol("zero connection epoch"));
        }
        let mut pipeline = Pipeline::over(stream, PipelineLimits::default(), budget)?;
        let (tag, root) = pipeline.attach(b"", b"")?;
        match pipeline.wait(tag, deadline)? {
            Reply::Attach(_) => {}
            Reply::Error(e) => return Err(PortalFileClientError::Remote(e)),
            _ => return Err(Protocol("attach reply")),
        }
        let mut client = Self {
            pipeline,
            root,
            journal: root,
            epoch,
            deadline,
            offset: 0,
            sequence: 0,
            submission: 0,
            pending: VecDeque::new(),
            grant: None,
            requested: false,
            max_payload: 0,
            broker_generation: 0,
        };
        let limits = client.open(&[b"limits"], 0)?;
        let tag = client
            .pipeline
            .read(limits, 0, PORTAL_FILE_MAX_OBJECT_BYTES as u32)?;
        let Reply::Read(bytes) = client.reply(tag)? else {
            return Err(Protocol("limits reply"));
        };
        let limits_record =
            decode_portal_file_limits(&bytes, epoch).map_err(|_| Protocol("limits record"))?;
        client.max_payload = limits_record.max_payload_bytes.into();
        client.broker_generation = limits_record.broker_generation;
        client.close(limits)?;
        client.journal = client.open(&[b"events"], 0)?;
        client.submit(PortalFileCandidate::Negotiate(PortalNegotiate {
            minimum_revision: 1,
            maximum_revision: 1,
        }))?;
        if !matches!(client.event()?, PortalFileEvent::Negotiated) {
            return Err(Protocol("negotiation refused"));
        }
        Ok(client)
    }

    pub fn request(&mut self, request: PortalRequest) -> Result<PortalBrokerResponseDecision> {
        if self.requested {
            return Err(Protocol("one transfer per connection"));
        }
        self.requested = true;
        let transfer = request.transfer.transfer;
        let expected = PortalGrant {
            transfer,
            source_namespace: request.transfer.source_namespace,
            target_namespace: request.transfer.target_namespace,
            kind: request.transfer.kind,
            source_generation: request.transfer.generation,
            broker_generation: self.broker_generation,
            deadline_msec: request.deadline_msec,
            state: PortalGrantState::Active,
        };
        self.submit(PortalFileCandidate::TransferRequest(request))?;
        let PortalFileEvent::Decision(packet) = self.event()? else {
            return Err(Protocol("decision expected"));
        };
        if packet.transfer != transfer {
            return Err(Protocol("decision transfer mismatch"));
        }
        if let PortalBrokerResponseDecision::Allowed(grant) = &packet.decision {
            if *grant != expected {
                return Err(Protocol(
                    "grant does not match request and owner generation",
                ));
            }
            self.grant = Some(grant.clone());
        }
        Ok(packet.decision)
    }

    /// Returns the recipient's terminal result, not the successful write or
    /// Submitted receipt. A failure drops further client authority to upload.
    pub fn upload(&mut self, payload: &[u8]) -> Result<PortalGrantState> {
        let grant = self.grant.take().ok_or(Protocol("no unused grant"))?;
        if payload.len() as u64 > self.max_payload {
            return Err(Protocol("payload bound"));
        }
        let total_bytes = payload.len() as u64;
        self.submit(PortalFileCandidate::PayloadBegin(PortalPayloadBegin {
            transfer: grant.transfer,
            total_bytes,
        }))?;
        let writer = self.open(&[b"upload", b"0"], 1)?;
        // Bounded chunks fit even the protocol's minimum negotiated msize.
        for (index, chunk) in payload.chunks(256).enumerate() {
            self.write(writer, (index * 256) as u64, chunk)?;
        }
        // Keep the writer alive through End: an earlier clunk revokes custody.
        self.submit(PortalFileCandidate::PayloadEnd(PortalPayloadEnd {
            transfer: grant.transfer,
            total_bytes,
        }))?;
        self.close(writer)?;
        let PortalFileEvent::TransferOutcome(outcome) = self.event()? else {
            return Err(Protocol("terminal outcome expected"));
        };
        if outcome.transfer != grant.transfer {
            return Err(Protocol("outcome transfer mismatch"));
        }
        Ok(outcome.state)
    }

    fn reply(&mut self, tag: Tag) -> Result<Reply> {
        match self.pipeline.wait(tag, self.deadline)? {
            Reply::Error(error) => Err(PortalFileClientError::Remote(error)),
            reply => Ok(reply),
        }
    }
    fn open(&mut self, names: &[&[u8]], flags: u32) -> Result<Fid> {
        let (tag, fid) = self.pipeline.walk(self.root, names)?;
        if !matches!(self.reply(tag)?, Reply::Walk(qids) if qids.len() == names.len()) {
            return Err(Protocol("incomplete walk"));
        }
        let tag = self.pipeline.lopen(fid, flags)?;
        if !matches!(self.reply(tag)?, Reply::Lopen { .. }) {
            return Err(Protocol("open reply"));
        }
        Ok(fid)
    }
    fn close(&mut self, fid: Fid) -> Result<()> {
        let tag = self.pipeline.clunk(fid)?;
        if !matches!(self.reply(tag)?, Reply::Clunk) {
            return Err(Protocol("clunk reply"));
        }
        Ok(())
    }
    fn write(&mut self, fid: Fid, offset: u64, bytes: &[u8]) -> Result<()> {
        let tag = self.pipeline.write(fid, offset, bytes)?;
        if !matches!(self.reply(tag)?, Reply::Write(n) if n as usize == bytes.len()) {
            return Err(Protocol("short control or payload write"));
        }
        Ok(())
    }
    fn submit(&mut self, candidate: PortalFileCandidate) -> Result<()> {
        self.submission = self
            .submission
            .checked_add(1)
            .ok_or(Protocol("submission overflow"))?;
        let bytes = encode_portal_file_candidate(self.epoch, self.submission, &candidate)
            .map_err(|_| Protocol("candidate encoding"))?;
        let tx = self.open(&[b"transaction"], 2)?;
        self.write(tx, 0, &bytes)?;
        let control = encode_portal_file_submit(PortalFileSubmit {
            connection_epoch: self.epoch,
            submission_id: self.submission,
            candidate_bytes: bytes.len() as u32,
        })
        .map_err(|_| Protocol("submit encoding"))?;
        let submit = self.open(&[b"submit"], 1)?;
        self.write(submit, 0, &control)?;
        self.close(submit)?;
        self.close(tx)?;
        match self.event()? {
            PortalFileEvent::Submitted(receipt)
                if receipt.submission_id == self.submission
                    && receipt.candidate_kind == candidate.kind() =>
            {
                Ok(())
            }
            _ => Err(Protocol("uncorrelated submission receipt")),
        }
    }
    fn event(&mut self) -> Result<PortalFileEvent> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(event);
        }
        let tag = self.pipeline.read(self.journal, self.offset, 512)?;
        let Reply::Read(bytes) = self.reply(tag)? else {
            return Err(Protocol("events reply"));
        };
        if bytes.is_empty() {
            return Err(Protocol("empty journal reply"));
        }
        self.offset = self
            .offset
            .checked_add(bytes.len() as u64)
            .ok_or(Protocol("journal offset overflow"))?;
        let mut remaining = bytes.as_slice();
        while !remaining.is_empty() {
            let size = remaining.get(..4).ok_or(Protocol("truncated event"))?;
            let size = u32::from_le_bytes(size.try_into().unwrap()) as usize;
            let record = remaining.get(..size).ok_or(Protocol("event length"))?;
            let (header, event) = decode_portal_file_event(record, self.epoch)
                .map_err(|_| Protocol("event encoding"))?;
            if self.sequence.checked_add(1) != Some(header.sequence) {
                return Err(Protocol("journal sequence"));
            }
            self.sequence = header.sequence;
            self.pending.push_back(event);
            remaining = &remaining[size..];
        }
        let ack = self.open(&[b"ack"], 1)?;
        let bytes = encode_portal_file_ack(PortalFileAck {
            connection_epoch: self.epoch,
            sequence: self.sequence,
        })
        .map_err(|_| Protocol("ack encoding"))?;
        self.write(ack, 0, &bytes)?;
        self.close(ack)?;
        self.pending.pop_front().ok_or(Protocol("missing event"))
    }
}
