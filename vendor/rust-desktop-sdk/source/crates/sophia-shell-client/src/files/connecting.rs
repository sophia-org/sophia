//! Caller-driven shell startup. One deadline and I/O budget cover every phase.
use super::*;
use crate::wire::Inbound;
use crate::{ShellClientOptions, ShellConnection};
use sophia_9p_client::pipeline::{
    IoBudget, IoInterest, PipelineError, PipelineLimits, VersionHandshake,
};
use sophia_9p_records::QidKind;
use sophia_shell_protocol::{ShellV1ClientHello, ShellV1ServerWelcome};
use std::os::{
    fd::{AsFd, BorrowedFd},
    unix::net::UnixStream,
};

#[derive(Clone, Copy)]
enum Node {
    Api,
    Events,
    Submit,
    Ack,
    Transaction,
    TransactionReuse,
    Limits,
}
impl Node {
    fn name(self) -> &'static [u8] {
        match self {
            Self::Api => b"api",
            Self::Events => b"events",
            Self::Submit => b"submit",
            Self::Ack => b"ack",
            Self::Transaction => b"transaction",
            Self::TransactionReuse => b"transaction-reuse",
            Self::Limits => b"limits",
        }
    }
    fn flags(self) -> u32 {
        match self {
            Self::Submit | Self::Ack => O_WRONLY,
            Self::Transaction | Self::TransactionReuse => O_RDWR,
            _ => O_RDONLY,
        }
    }
}
#[derive(Clone, Copy)]
enum Stage {
    Attach,
    Walk(Node),
    Open(Node),
    Api,
    ClunkApi,
    Candidate,
    Submit,
    ClunkTransaction,
    Events,
    Ack,
    Limits,
    ClunkLimits,
    Ready,
}

/// Nonblocking startup from a connected socket or an authenticated sole root.
/// Time is caller-monotonic milliseconds. The explicit absolute deadline governs
/// startup; `ShellClientOptions::handshake_timeout` belongs to blocking helpers.
/// Constructors queue no socket traffic. Keep the authentication relay alive.
pub struct Connecting {
    version: Option<VersionHandshake>,
    handshake: Option<Handshake>,
    result: Option<ShellConnection>,
    options: ShellClientOptions,
    now: u64,
    deadline: u64,
    error: Option<ShellClientError>,
}
impl Connecting {
    pub fn direct(
        stream: UnixStream,
        limits: PipelineLimits,
        options: ShellClientOptions,
        now: u64,
        deadline: u64,
    ) -> Result<Self, ShellClientError> {
        validate(options, now, deadline)?;
        Ok(Self {
            version: Some(VersionHandshake::new(stream, limits, now, deadline)?),
            handshake: None,
            result: None,
            options,
            now,
            deadline,
            error: None,
        })
    }
    pub fn attached(
        pipeline: Pipeline,
        root: Fid,
        options: ShellClientOptions,
        now: u64,
        deadline: u64,
    ) -> Result<Self, ShellClientError> {
        validate(options, now, deadline)?;
        if !pipeline.is_quiescent() || pipeline.fid_count() != 1 || !pipeline.owns_fid(root) {
            return Err(ShellClientError::Protocol(
                "shell handoff requires a quiescent sole root",
            ));
        }
        Ok(Self {
            version: None,
            handshake: Some(Handshake::new(pipeline, Some(root), options)?),
            result: None,
            options,
            now,
            deadline,
            error: None,
        })
    }
    pub fn ready(&self) -> bool {
        self.result.is_some() && self.error.is_none()
    }
    pub fn error(&self) -> Option<&ShellClientError> {
        self.error.as_ref()
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        if let Some(v) = &self.version {
            v.fd()
        } else if let Some(h) = &self.handshake {
            Some(h.pipeline.as_fd())
        } else {
            self.result.as_ref().map(AsFd::as_fd)
        }
    }
    pub fn io_interest(&self) -> Result<IoInterest, ShellClientError> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if let Some(v) = &self.version {
            return Ok(v.io_interest());
        }
        if let Some(h) = &self.handshake {
            let mut interest = h.pipeline.io_interest()?;
            interest.immediate |= h.pending.is_none() && h.retry_at.is_none_or(|t| self.now >= t);
            return Ok(interest);
        }
        if self.ready() {
            return Ok(IoInterest {
                readable: false,
                writable: false,
                immediate: true,
            });
        }
        Err(ShellClientError::PeerClosed)
    }
    pub fn wake_deadline(&self) -> Option<u64> {
        if self.error.is_some()
            || self.ready()
            || (self.version.is_none() && self.handshake.is_none())
        {
            None
        } else {
            Some(
                self.handshake
                    .as_ref()
                    .and_then(|h| h.retry_at)
                    .unwrap_or(self.deadline)
                    .min(self.deadline),
            )
        }
    }
    pub fn take_connection(&mut self) -> Option<ShellConnection> {
        self.result.take()
    }
    fn fail(&mut self, e: ShellClientError) -> ShellClientError {
        self.version.take();
        self.handshake.take();
        self.result.take();
        self.error.get_or_insert(e).clone()
    }
    pub fn close(&mut self) {
        self.fail(ShellClientError::PeerClosed);
    }
    /// One shared byte/syscall allowance, at most 32 rounds, and no clock reads.
    /// Clock regression leaves state unchanged; expiry closes without more I/O.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), ShellClientError> {
        if now < self.now {
            return Err(ShellClientError::InvalidArgument("clock regressed"));
        }
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.now = now;
        if self.ready() {
            return Ok(());
        }
        if now >= self.deadline {
            return Err(self.fail(PipelineError::Timeout.into()));
        }
        match self.service(now, bytes) {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }
    fn service(&mut self, now: u64, bytes: usize) -> Result<(), ShellClientError> {
        let mut budget = IoBudget::new(bytes);
        if let Some(v) = &mut self.version {
            v.poll_with_budget(now, &mut budget)?;
            let Some(pipeline) = v.take_pipeline() else {
                return Ok(());
            };
            self.version = None;
            self.handshake = Some(Handshake::new(pipeline, None, self.options)?);
        }
        for _ in 0..32 {
            let h = self
                .handshake
                .as_mut()
                .ok_or(ShellClientError::PeerClosed)?;
            h.queue(now)?;
            let io = h.pipeline.poll_with_budget(&mut budget);
            // A validated reply may precede EOF in the same read pass.
            let reply = h.pipeline.take_reply();
            if let Some((tag, reply)) = reply {
                if h.pending.take() != Some(tag) {
                    return Err(ShellClientError::Protocol("unowned startup reply"));
                }
                h.accept(reply, now)?;
            } else {
                io?;
                break;
            }
            io?;
            if matches!(h.stage, Stage::Ready) {
                self.result = Some(self.handshake.take().unwrap().finish()?);
                break;
            }
            if budget.exhausted() {
                break;
            }
        }
        Ok(())
    }
}
fn validate(options: ShellClientOptions, now: u64, deadline: u64) -> Result<(), ShellClientError> {
    if options.minimum_revision == 0 || options.minimum_revision > options.maximum_revision {
        return Err(ShellClientError::UnsupportedRevision);
    }
    if deadline <= now {
        return Err(PipelineError::Timeout.into());
    }
    Ok(())
}
struct Handshake {
    pipeline: Pipeline,
    options: ShellClientOptions,
    stage: Stage,
    pending: Option<Tag>,
    root: Fid,
    current: Fid,
    events: Fid,
    submit: Fid,
    ack: Fid,
    iounit: u32,
    epoch: u64,
    descriptor: bool,
    native: bool,
    bytes: Vec<u8>,
    candidate: Vec<u8>,
    written: usize,
    sent: u32,
    events_buffer: Vec<u8>,
    offset: u64,
    sequence: u64,
    submitted: bool,
    negotiated: Option<ShellFileNegotiated>,
    refusal: Option<sophia_shell_protocol::ContentAdmissionRefused>,
    retry_at: Option<u64>,
    retry_delay: u64,
    inbox: VecDeque<Inbound>,
    upload_slots: u8,
    transaction_fid: Option<Fid>,
}
impl Handshake {
    fn new(
        pipeline: Pipeline,
        root: Option<Fid>,
        options: ShellClientOptions,
    ) -> Result<Self, ShellClientError> {
        if pipeline.is_poisoned()
            || pipeline.limits().max_outstanding < 8
            || pipeline.limits().max_fids < 32
        {
            return Err(ShellClientError::Protocol(
                "shell pipeline needs eight requests and 32 fids",
            ));
        }
        Ok(Self {
            pipeline,
            options,
            stage: if root.is_some() {
                Stage::Walk(Node::Api)
            } else {
                Stage::Attach
            },
            pending: None,
            root: root.unwrap_or(Fid::NOFID),
            current: Fid::NOFID,
            events: Fid::NOFID,
            submit: Fid::NOFID,
            ack: Fid::NOFID,
            iounit: 0,
            epoch: 0,
            descriptor: false,
            native: false,
            bytes: Vec::new(),
            candidate: Vec::new(),
            written: 0,
            sent: 0,
            events_buffer: Vec::new(),
            offset: 0,
            sequence: 0,
            submitted: false,
            negotiated: None,
            refusal: None,
            retry_at: None,
            retry_delay: 4,
            inbox: VecDeque::new(),
            upload_slots: 0,
            transaction_fid: None,
        })
    }
    fn queue(&mut self, now: u64) -> Result<(), ShellClientError> {
        if self.pending.is_some() || self.retry_at.is_some_and(|t| now < t) {
            return Ok(());
        }
        self.retry_at = None;
        if matches!(self.stage, Stage::Events) && self.event()? {
            self.stage = Stage::Ack;
        }
        let w = &mut self.pipeline;
        let tag = match self.stage {
            Stage::Attach => {
                let (tag, fid) = w.attach(b"", b"")?;
                self.root = fid;
                tag
            }
            Stage::Walk(node) => {
                let (tag, fid) = w.walk(self.root, &[node.name()])?;
                self.current = fid;
                tag
            }
            Stage::Open(node) => w.lopen(self.current, node.flags())?,
            Stage::Api | Stage::Limits => {
                let cap = if matches!(self.stage, Stage::Api) {
                    crate::SHELL_FILES_API_LINE_MAX_BYTES as usize
                } else {
                    SHELL_FILE_MAX_OBJECT_BYTES
                };
                let count = (cap - self.bytes.len())
                    .max(1)
                    .min((w.msize() - 11) as usize) as u32;
                w.read(
                    self.current,
                    self.bytes.len() as u64,
                    if self.iounit == 0 {
                        count
                    } else {
                        count.min(self.iounit)
                    },
                )?
            }
            Stage::ClunkApi | Stage::ClunkTransaction | Stage::ClunkLimits => {
                w.clunk(self.current)?
            }
            Stage::Candidate => {
                let count = (w.msize() - 23).min(if self.iounit == 0 {
                    u32::MAX
                } else {
                    self.iounit
                });
                self.sent = count.min((self.candidate.len() - self.written) as u32);
                w.write(
                    self.current,
                    self.written as u64,
                    &self.candidate[self.written..self.written + self.sent as usize],
                )?
            }
            Stage::Submit => w.write(
                self.submit,
                0,
                &encode_shell_file_submit(ShellFileSubmit {
                    connection_epoch: self.epoch,
                    submission_id: 1,
                    candidate_bytes: self.candidate.len() as u32,
                })?,
            )?,
            Stage::Events => w.read(self.events, self.offset, w.msize() - 11)?,
            Stage::Ack => w.write(
                self.ack,
                0,
                &encode_shell_file_ack(ShellFileAck {
                    connection_epoch: self.epoch,
                    sequence: self.sequence,
                })?,
            )?,
            Stage::Ready => return Ok(()),
        };
        self.pending = Some(tag);
        Ok(())
    }
    fn accept(&mut self, reply: Reply, now: u64) -> Result<(), ShellClientError> {
        if let Reply::Error(errno) = reply {
            if matches!(self.stage, Stage::Walk(Node::TransactionReuse)) && errno.0 == 2 {
                self.stage = Stage::Ready;
                return Ok(());
            }
            if matches!(self.stage, Stage::Submit) && errno.0 == 11 {
                self.retry_at = Some(now.saturating_add(self.retry_delay));
                self.retry_delay = (self.retry_delay * 2).min(256);
                return Ok(());
            }
            return Err(PipelineError::Remote(errno).into());
        }
        self.stage = match (self.stage, reply) {
            (Stage::Attach, Reply::Attach(q)) if q.kind == QidKind::Directory => {
                Stage::Walk(Node::Api)
            }
            (Stage::Walk(node), Reply::Walk(q)) if q.len() == 1 && q[0].kind == QidKind::File => {
                Stage::Open(node)
            }
            (Stage::Open(node), Reply::Lopen { qid, iounit }) if qid.kind == QidKind::File => {
                self.iounit = iounit;
                match node {
                    Node::Api => Stage::Api,
                    Node::Events => {
                        self.events = self.current;
                        Stage::Walk(Node::Submit)
                    }
                    Node::Submit => {
                        if iounit != 0 && iounit < 24 {
                            return Err(ShellClientError::Protocol("submit iounit too small"));
                        }
                        self.submit = self.current;
                        Stage::Walk(Node::Ack)
                    }
                    Node::Ack => {
                        if iounit != 0 && iounit < 16 {
                            return Err(ShellClientError::Protocol("ack iounit too small"));
                        }
                        self.ack = self.current;
                        Stage::Walk(Node::Transaction)
                    }
                    Node::Transaction => Stage::Candidate,
                    Node::TransactionReuse => {
                        self.transaction_fid = Some(self.current);
                        Stage::Ready
                    }
                    Node::Limits => {
                        self.bytes.clear();
                        Stage::Limits
                    }
                }
            }
            (Stage::Api, Reply::Read(data)) => {
                if data.is_empty() {
                    return Err(ShellClientError::Protocol("incomplete api"));
                }
                self.bytes.extend_from_slice(&data);
                if self.bytes.len() >= crate::SHELL_FILES_API_LINE_MAX_BYTES as usize {
                    return Err(ShellClientError::Protocol("api over cap"));
                }
                if self.bytes.contains(&b'\n') {
                    let (epoch, role) = crate::parse_files_api(&self.bytes)?;
                    self.epoch = epoch;
                    self.descriptor = role == "descriptor";
                    self.native = role == "launcher";
                    self.candidate = encode_shell_file_negotiate(
                        ShellFileHeader {
                            kind: ShellFileKind::Negotiate,
                            connection_epoch: epoch,
                            submission_id: 1,
                            sequence: 0,
                        },
                        ShellV1ClientHello {
                            minimum_revision: self.options.minimum_revision,
                            maximum_revision: self.options.maximum_revision,
                            required_capabilities: self.options.required_capabilities,
                        },
                    )?;
                    Stage::ClunkApi
                } else {
                    Stage::Api
                }
            }
            (Stage::ClunkApi, Reply::Clunk) => Stage::Walk(Node::Events),
            (Stage::Candidate, Reply::Write(n)) if n > 0 && n <= self.sent => {
                self.written += n as usize;
                if self.written == self.candidate.len() {
                    Stage::Submit
                } else {
                    Stage::Candidate
                }
            }
            (Stage::Submit, Reply::Write(24)) => Stage::ClunkTransaction,
            (Stage::ClunkTransaction, Reply::Clunk) => Stage::Events,
            (Stage::Events, Reply::Read(data)) => {
                if data.is_empty() {
                    return Err(ShellClientError::PeerClosed);
                }
                self.offset = self
                    .offset
                    .checked_add(data.len() as u64)
                    .ok_or(ShellClientError::Protocol("event offset overflow"))?;
                if self.events_buffer.len() + data.len()
                    > SHELL_FILE_MAX_TRANSACTION_BYTES + self.pipeline.msize() as usize
                {
                    return Err(ShellClientError::Protocol("handshake journal over cap"));
                }
                self.events_buffer.extend_from_slice(&data);
                Stage::Events
            }
            (Stage::Ack, Reply::Write(16)) => {
                if let Some(refused) = self.refusal.take() {
                    return Err(ShellClientError::AdmissionRefused(refused));
                }
                if let Some(n) = &self.negotiated {
                    if n.limits_published {
                        Stage::Walk(Node::Limits)
                    } else {
                        Stage::Walk(Node::TransactionReuse)
                    }
                } else {
                    Stage::Events
                }
            }
            (Stage::Limits, Reply::Read(data)) => {
                if self.bytes.len() + data.len() > SHELL_FILE_MAX_OBJECT_BYTES {
                    return Err(ShellClientError::Protocol("Limits over cap"));
                }
                if data.is_empty() {
                    if decode_shell_file_record(&self.bytes, ShellFileClass::Object)?
                        .header
                        .connection_epoch
                        != self.epoch
                    {
                        return Err(ShellClientError::Protocol(
                            "Limits from another connection epoch",
                        ));
                    }
                    let limits = decode_shell_file_limits(&self.bytes)?;
                    if limits.grant.connection_epoch != self.epoch {
                        return Err(ShellClientError::Protocol(
                            "Limits grant from another connection epoch",
                        ));
                    }
                    self.upload_slots = limits
                        .max_open_transfers
                        .min(u32::from(SHELL_FILE_MAX_UPLOAD_SLOTS))
                        as u8;
                    self.inbox.push_back(Inbound::Content(
                        TransactionId::INVALID,
                        ShellContentRecord::Limits(limits),
                    ));
                    Stage::ClunkLimits
                } else {
                    self.bytes.extend_from_slice(&data);
                    Stage::Limits
                }
            }
            (Stage::ClunkLimits, Reply::Clunk) => Stage::Walk(Node::TransactionReuse),
            _ => return Err(ShellClientError::Protocol("unexpected shell startup reply")),
        };
        Ok(())
    }
    fn event(&mut self) -> Result<bool, ShellClientError> {
        if self.events_buffer.len() < 4 {
            return Ok(false);
        }
        let size = u32::from_le_bytes(self.events_buffer[..4].try_into().unwrap()) as usize;
        if !(SHELL_FILE_HEADER_BYTES..=SHELL_FILE_MAX_TRANSACTION_BYTES).contains(&size) {
            return Err(ShellClientError::Protocol("handshake record length"));
        }
        if self.events_buffer.len() < size {
            return Ok(false);
        }
        let record = &self.events_buffer[..size];
        let parsed = decode_shell_file_record(record, ShellFileClass::Event)?;
        if parsed.header.connection_epoch != self.epoch || parsed.header.sequence <= self.sequence {
            return Err(ShellClientError::Protocol("handshake event identity"));
        }
        match parsed.header.kind {
            ShellFileKind::Submitted => {
                let s = decode_shell_file_submitted(record)?;
                if self.submitted
                    || s.submission_id != 1
                    || s.candidate_kind != ShellFileKind::Negotiate
                {
                    return Err(ShellClientError::Protocol(
                        "unexpected Submitted before negotiation",
                    ));
                }
                self.submitted = true;
            }
            ShellFileKind::Negotiated => {
                if !self.submitted {
                    return Err(ShellClientError::Protocol(
                        "Negotiated before bootstrap custody",
                    ));
                }
                let n = decode_shell_file_negotiated(record)?;
                if n.welcome.selected_revision < self.options.minimum_revision
                    || n.welcome.selected_revision > self.options.maximum_revision
                {
                    return Err(ShellClientError::UnsupportedRevision);
                }
                if n.welcome.capabilities & self.options.required_capabilities
                    != self.options.required_capabilities
                {
                    return Err(ShellClientError::MissingCapability);
                }
                if self.descriptor {
                    super::descriptor::validate_negotiated(&self.options, &n)?;
                }
                super::native_launcher::validate_negotiated(self.native, &self.options, &n)?;
                self.negotiated = Some(n);
            }
            ShellFileKind::Refused => self.refusal = Some(decode_shell_file_refused(record)?),
            _ => return Err(ShellClientError::Protocol("unexpected bootstrap event")),
        }
        self.sequence = parsed.header.sequence;
        self.events_buffer.drain(..size);
        Ok(true)
    }
    fn finish(self) -> Result<ShellConnection, ShellClientError> {
        let welcome: ShellV1ServerWelcome = self
            .negotiated
            .ok_or(ShellClientError::Protocol("missing negotiation"))?
            .welcome;
        let wire = FileWire {
            pipeline: self.pipeline,
            epoch: self.epoch,
            capabilities: welcome.capabilities,
            descriptor: self.descriptor,
            native_launcher: self.native,
            root: self.root,
            events_fid: self.events,
            submit_fid: self.submit,
            ack_fid: self.ack,
            reusable_transactions: self.transaction_fid.is_some(),
            transaction_fid: self.transaction_fid,
            upload_slots: self.upload_slots,
            next_submission_id: 2,
            read_tag: None,
            read_offset: self.offset,
            event_buf: self.events_buffer,
            reply_scratch: Vec::new(),
            ack_ready: self.sequence,
            acked: self.sequence,
            ack_confirmed: self.sequence,
            automatic_ack: true,
            automatic_objects: true,
            automatic_events: true,
            retained_event: None,
            retained_event_consumed: false,
            ack_tag: None,
            object_fetch: None,
            snapshot_failure: None,
            pending_objects: [None; super::FEEDS.len()],
            verified_objects: [None; super::FEEDS.len()],
            attempted_objects: [false; super::FEEDS.len()],
            holds: [None; super::FEEDS.len()],
            progress: 0,
            pass: 0,
            service_pending: true,
            dispatch_time: None,
            last_dispatch_time: None,
            early_submitted: None,
            submitted_sequence: 0,
            pending: VecDeque::new(),
            staged: Vec::new(),
            staged_begins: Default::default(),
            current: None,
            backoff: super::RETRY_FIRST,
            custody: Vec::new(),
            retire: 0,
            uploads: Default::default(),
            forgettable: Default::default(),
            peer_closed: false,
            event_fault: false,
            fatal: None,
        };
        Ok(ShellConnection {
            wire: Box::new(wire),
            welcome,
            inbox: self.inbox,
            output: crate::outbox::ClientOutbox::default(),
            ledger: crate::custody::Ledger::default(),
        })
    }
}
