//! Raw 9P client for the ring peer: one authenticated connection, journal
//! records classified in arrival order (D4E1 owned events beside shell-file
//! records), with envelope epoch and monotonic-sequence validation.
use sophia_9p_client::pipeline::{Pipeline, Reply};
use sophia_9p_client::records::{Fid, Tag};
use sophia_protocol::{shell_files::*, *};
use sophia_runtime::owned_ring_protocol::Event;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Protocol codec errors are Debug-only; box them as messages.
pub trait Codec<T> {
    fn codec(self) -> Result<T>;
}
impl<T, E: std::fmt::Debug> Codec<T> for std::result::Result<T, E> {
    fn codec(self) -> Result<T> {
        self.map_err(|e| format!("{e:?}").into())
    }
}

const WAIT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum Inbound {
    Owned(Event),
    Submitted(u64, u64),
    Negotiated(ShellV1ServerWelcome),
    ChannelReady(u64),
    Published(ShellFileKind),
    Refused(u16),
    Facts(ContentOutputFacts),
    Allocation(ContentAllocationResult),
    Status(TransactionId, ContentResourceStatus),
    Released(TransactionId, ContentResourceReleased),
    Permit(ContentFramePermit),
    Outcome(ContentCandidateOutcome),
}

pub struct Client {
    pipe: Pipeline,
    root: Fid,
    events: Fid,
    submit: Fid,
    ack: Fid,
    buf: Vec<u8>,
    offset: u64,
    queued: VecDeque<Inbound>,
    epoch: u64,
    last_sequence: u64,
    acked: u64,
}

impl Client {
    pub fn new(pipe: Pipeline, root: Fid) -> Self {
        Self {
            pipe,
            root,
            events: Fid(0),
            submit: Fid(0),
            ack: Fid(0),
            buf: Vec::new(),
            offset: 0,
            queued: VecDeque::new(),
            epoch: 0,
            last_sequence: 0,
            acked: 0,
        }
    }

    /// Open the fixed files and learn the authenticated export epoch from
    /// the `api` line, exactly where the SDK learns it.
    pub fn init(&mut self) -> Result<()> {
        self.events = self.open(b"events", 0)?;
        self.submit = self.open(b"submit", 1)?;
        self.ack = self.open(b"ack", 1)?;
        let api = self.open(b"api", 0)?;
        let bytes = self.read(api, 0, 4096)?;
        self.clunk(api)?;
        let text = std::str::from_utf8(&bytes)?;
        let line = text.strip_suffix('\n').ok_or("api line missing newline")?;
        let mut fields = line.split(' ');
        if fields.next() != Some("sophia-shell-files") {
            return Err("api line missing family".into());
        }
        let version: u16 = fields
            .next()
            .and_then(|f| f.strip_prefix("version="))
            .and_then(|v| v.parse().ok())
            .ok_or("api line missing version")?;
        if version != SHELL_FILE_API_VERSION {
            return Err("api line version mismatch".into());
        }
        let role = fields
            .next()
            .and_then(|f| f.strip_prefix("role="))
            .filter(|r| !r.is_empty())
            .ok_or("api line missing role")?;
        if role != "bar" {
            return Err("api line role mismatch".into());
        }
        let epoch: u64 = fields
            .next()
            .and_then(|f| f.strip_prefix("epoch="))
            .and_then(|v| v.parse().ok())
            .filter(|e| *e != 0)
            .ok_or("api line missing nonzero epoch")?;
        if fields.next() != Some("fd_transfer=none") || fields.next().is_some() {
            return Err("api line shape mismatch".into());
        }
        self.epoch = epoch;
        Ok(())
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    fn wait(&mut self, tag: Tag) -> Result<Reply> {
        Ok(self.pipe.wait(tag, Instant::now() + WAIT)?)
    }

    pub fn open(&mut self, name: &[u8], flags: u32) -> Result<Fid> {
        let (tag, fid) = self.pipe.walk(self.root, &[name])?;
        match self.wait(tag)? {
            Reply::Walk(qids) if qids.len() == 1 => {}
            Reply::Error(errno) => {
                return Err(format!("walk {}: {errno:?}", String::from_utf8_lossy(name)).into());
            }
            other => return Err(format!("unexpected walk reply: {other:?}").into()),
        }
        let tag = self.pipe.lopen(fid, flags)?;
        match self.wait(tag)? {
            Reply::Lopen { .. } => Ok(fid),
            Reply::Error(errno) => {
                Err(format!("open {}: {errno:?}", String::from_utf8_lossy(name)).into())
            }
            other => Err(format!("unexpected open reply: {other:?}").into()),
        }
    }

    pub fn write(&mut self, fid: Fid, bytes: &[u8]) -> Result<()> {
        let tag = self.pipe.write(fid, 0, bytes)?;
        match self.wait(tag)? {
            Reply::Write(count) if count as usize == bytes.len() => Ok(()),
            Reply::Error(errno) => Err(format!("write fid {}: {errno:?}", fid.0).into()),
            other => Err(format!("unexpected write reply: {other:?}").into()),
        }
    }

    pub fn read(&mut self, fid: Fid, offset: u64, count: u32) -> Result<Vec<u8>> {
        let tag = self.pipe.read(fid, offset, count)?;
        match self.wait(tag)? {
            Reply::Read(bytes) => Ok(bytes),
            Reply::Error(errno) => Err(format!("read: {errno:?}").into()),
            other => Err(format!("unexpected read reply: {other:?}").into()),
        }
    }

    /// Read a published object snapshot: whole records, served in one
    /// reply; a short read ends it (a read at the end would park).
    fn read_object(&mut self, name: &[u8]) -> Result<Vec<u8>> {
        let fid = self.open(name, 0)?;
        let mut value = Vec::new();
        loop {
            let part = self.read(fid, value.len() as u64, 65500)?;
            let short = part.len() < 65500;
            value.extend_from_slice(&part);
            if value.len() > 1 << 20 {
                return Err("object exceeded peer bound".into());
            }
            if short {
                break;
            }
        }
        self.clunk(fid)?;
        Ok(value)
    }

    pub fn fetch_limits(&mut self) -> Result<ContentLimits> {
        let bytes = self.read_object(b"limits")?;
        decode_shell_file_limits(&bytes).codec()
    }

    pub fn fetch_facts(&mut self) -> Result<ContentOutputFacts> {
        let bytes = self.read_object(b"outputs")?;
        match decode_shell_file_outputs(&bytes).codec()?.record {
            ShellContentRecord::OutputFacts(facts) => Ok(facts),
            _ => Err("outputs record mismatch".into()),
        }
    }

    fn clunk(&mut self, fid: Fid) -> Result<()> {
        let tag = self.pipe.clunk(fid)?;
        match self.wait(tag)? {
            Reply::Clunk => Ok(()),
            Reply::Error(errno) => Err(format!("clunk: {errno:?}").into()),
            other => Err(format!("unexpected clunk reply: {other:?}").into()),
        }
    }

    /// Journal acknowledgement is cumulative: acking a sequence covers
    /// every earlier one, and a non-advancing ack is refused by the
    /// server. Records may be consumed out of order (a Submitted arrives
    /// before its response), so only ever advance the watermark.
    fn ack(&mut self, sequence: u64) -> Result<()> {
        if sequence <= self.acked {
            return Ok(());
        }
        let bytes = encode_shell_file_ack(ShellFileAck {
            connection_epoch: self.epoch,
            sequence,
        })
        .codec()?;
        self.write(self.ack, &bytes)?;
        self.acked = sequence;
        Ok(())
    }

    fn check_sequence(&mut self, sequence: u64) -> Result<()> {
        if sequence <= self.last_sequence {
            return Err("journal sequence did not rise".into());
        }
        self.last_sequence = sequence;
        Ok(())
    }

    fn classify(&mut self, bytes: &[u8]) -> Result<Inbound> {
        if bytes.get(4..8) == Some(b"D4E1") {
            let event = Event::decode(bytes).ok_or("malformed owned ring event")?;
            self.check_sequence(event.sequence)?;
            self.ack(event.sequence)?;
            return Ok(Inbound::Owned(event));
        }
        let parsed = decode_shell_file_record_at_revision(bytes, ShellFileClass::Event, 10)
            .map_err(|e| format!("journal record: {e:?}"))?;
        if parsed.header.connection_epoch != self.epoch {
            return Err("journal record from another connection epoch".into());
        }
        self.check_sequence(parsed.header.sequence)?;
        let sequence = parsed.header.sequence;
        let inbound = match parsed.header.kind {
            ShellFileKind::Submitted => Inbound::Submitted(
                sequence,
                decode_shell_file_submitted_at_revision(bytes, 10)
                    .codec()?
                    .submission_id,
            ),
            ShellFileKind::Negotiated => {
                Inbound::Negotiated(decode_shell_file_negotiated(bytes).codec()?.welcome)
            }
            ShellFileKind::ChannelReady => Inbound::ChannelReady(u64::from(
                ShellChannelReady::decode(parsed.body)
                    .codec()?
                    .granted_source_kinds,
            )),
            ShellFileKind::Refused => {
                Inbound::Refused(decode_shell_file_refused(bytes).codec()?.reason)
            }
            ShellFileKind::ObjectPublished => {
                Inbound::Published(decode_shell_file_object_published(bytes).codec()?.object)
            }
            ShellFileKind::Outputs => match decode_shell_file_outputs(bytes).codec()?.record {
                ShellContentRecord::OutputFacts(facts) => Inbound::Facts(facts),
                _ => return Err("outputs record mismatch".into()),
            },
            ShellFileKind::AllocationResult => {
                match decode_shell_file_allocation_result(bytes).codec()?.record {
                    ShellContentRecord::AllocationResult(result) => Inbound::Allocation(result),
                    _ => return Err("allocation record mismatch".into()),
                }
            }
            ShellFileKind::ResourceStatus => {
                let decoded = decode_shell_file_resource_status_at_revision(bytes, 10).codec()?;
                match decoded.record {
                    ShellContentRecord::ResourceStatus(status) => {
                        Inbound::Status(decoded.transaction, status)
                    }
                    _ => return Err("status record mismatch".into()),
                }
            }
            ShellFileKind::ResourceReleased => {
                let decoded = decode_shell_file_resource_released(bytes).codec()?;
                match decoded.record {
                    ShellContentRecord::ResourceReleased(released) => {
                        Inbound::Released(decoded.transaction, released)
                    }
                    _ => return Err("released record mismatch".into()),
                }
            }
            ShellFileKind::FramePermit => {
                match decode_shell_file_transaction(bytes, ShellFileKind::FramePermit)
                    .codec()?
                    .record
                {
                    ShellContentRecord::FramePermit(permit) => Inbound::Permit(permit),
                    _ => return Err("permit record mismatch".into()),
                }
            }
            ShellFileKind::CandidateOutcome => {
                match decode_shell_file_transaction(bytes, ShellFileKind::CandidateOutcome)
                    .codec()?
                    .record
                {
                    ShellContentRecord::CandidateOutcome(outcome) => Inbound::Outcome(outcome),
                    _ => return Err("outcome record mismatch".into()),
                }
            }
            kind => return Err(format!("unexpected journal kind {kind:?}").into()),
        };
        if !matches!(inbound, Inbound::Submitted(..)) {
            self.ack(sequence)?;
        }
        Ok(inbound)
    }

    pub fn take(&mut self) -> Result<Inbound> {
        loop {
            if let Some(inbound) = self.queued.pop_front() {
                return Ok(inbound);
            }
            while self.buf.len() >= 4 {
                let size = u32::from_le_bytes(self.buf[..4].try_into().unwrap()) as usize;
                if size < 4 || self.buf.len() < size {
                    break;
                }
                let record: Vec<u8> = self.buf.drain(..size).collect();
                let inbound = self.classify(&record)?;
                self.queued.push_back(inbound);
            }
            if self.queued.is_empty() {
                let bytes = self.read(self.events, self.offset, 65500)?;
                if bytes.is_empty() {
                    return Err("events read made no progress".into());
                }
                self.offset += bytes.len() as u64;
                self.buf.extend_from_slice(&bytes);
            }
        }
    }

    /// Stage one candidate record and settle on its Submitted event; other
    /// journal records stay queued in arrival order.
    pub fn submit(&mut self, bytes: &[u8], submission_id: u64) -> Result<()> {
        let transaction = self.open(b"transaction", 2)?;
        self.write(transaction, bytes)?;
        let submit = encode_shell_file_submit(ShellFileSubmit {
            connection_epoch: self.epoch,
            submission_id,
            candidate_bytes: bytes.len() as u32,
        })
        .codec()?;
        self.write(self.submit, &submit)?;
        let mut stash = VecDeque::new();
        loop {
            match self.take()? {
                Inbound::Submitted(sequence, id) if id == submission_id => {
                    self.ack(sequence)?;
                    break;
                }
                Inbound::Refused(reason) => {
                    return Err(
                        format!("submission {submission_id} refused: reason {reason}").into(),
                    );
                }
                other => stash.push_back(other),
            }
        }
        stash.extend(self.queued.drain(..));
        self.queued = stash;
        self.clunk(transaction)
    }
}
