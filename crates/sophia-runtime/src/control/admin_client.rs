use sophia_9p_client::pipeline::{Pipeline, PipelineError, PipelineLimits, Reply};
use sophia_9p_client::records::{Fid, Tag};
use sophia_protocol::admin_files::*;
use sophia_protocol::{ControlCatalog, ControlCommand, ControlOutcome, ControlOwner};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

const FRAME_TIMEOUT: Duration = Duration::from_secs(2);

/// Bounded administrative file client. A failed transport is never reconnected
/// or replayed; invocation_pending remains true when effects are uncertain.
pub struct AdminClient {
    wire: Option<Pipeline>,
    root: Fid,
    ctl: Fid,
    info: AdminInfo,
    catalog: Option<ControlCatalog>,
    next_request: u64,
    last_request: u64,
    invocation_pending: bool,
}

impl AdminClient {
    pub fn connect(path: &Path) -> io::Result<Self> {
        let stream = super::client::connect_stream(path)?;
        let limits = PipelineLimits {
            msize: 65536,
            max_outstanding: 2,
            max_fids: 4,
            max_buffered_output: 65536,
            max_buffered_input: 65536,
        };
        let mut wire = Pipeline::over(stream, limits, FRAME_TIMEOUT).map_err(wire_error)?;
        let deadline = Instant::now() + FRAME_TIMEOUT;
        let (tag, root) = wire.attach(b"", b"").map_err(wire_error)?;
        if !matches!(
            wire.wait(tag, deadline).map_err(wire_error)?,
            Reply::Attach(_)
        ) {
            return Err(invalid("administrative attach refused"));
        }
        let bytes = snapshot(&mut wire, root, b"info", ADMIN_LINE_MAX_BYTES, deadline)?;
        let info = decode_admin_info(&bytes).map_err(|_| invalid("invalid administrative info"))?;
        // A peer cannot turn a bounded command into an arbitrarily long wait.
        if info.command_timeout_ms > 10000
            || info.frame_timeout_ms > 2000
            || info.idle_timeout_ms > 60000
        {
            return Err(invalid("unsupported administrative deadlines"));
        }
        let ctl = open(&mut wire, root, b"ctl", 1, deadline)?;
        Ok(Self {
            wire: Some(wire),
            root,
            ctl,
            info,
            catalog: None,
            next_request: 1,
            last_request: 0,
            invocation_pending: false,
        })
    }

    pub fn info(&self) -> AdminInfo {
        self.info
    }
    pub fn request_id(&self) -> u64 {
        self.last_request
    }
    pub fn invocation_pending(&self) -> bool {
        self.invocation_pending
    }

    pub fn commands(&mut self) -> io::Result<ControlCatalog> {
        let result = self.commands_inner();
        self.close_on_error(result)
    }

    fn commands_inner(&mut self) -> io::Result<ControlCatalog> {
        let deadline = Instant::now() + Duration::from_millis(self.info.frame_timeout_ms.into());
        let root = self.root;
        let bytes = snapshot(
            self.wire()?,
            root,
            b"commands",
            ADMIN_CATALOG_MAX_BYTES,
            deadline,
        )?;
        let catalog =
            decode_admin_catalog(&bytes).map_err(|_| invalid("invalid administrative catalog"))?;
        self.catalog = Some(catalog.clone());
        Ok(catalog)
    }

    pub fn invoke(&mut self, command: ControlCommand) -> io::Result<(u64, ControlOutcome, String)> {
        // Discovery and local validation happen before any mutation is queued.
        let catalog = match &self.catalog {
            Some(c) => c.clone(),
            None => self.commands()?,
        };
        if !catalog.commands.contains(&command) {
            return Err(invalid("command is not advertised by this session"));
        }
        let request = self.next_request;
        self.next_request = request
            .checked_add(1)
            .ok_or_else(|| invalid("request IDs exhausted"))?;
        let bytes = encode_admin_invocation(&AdminInvocation {
            connection: self.info.connection,
            request,
            generation: catalog.generation,
            command: command.clone(),
        })
        .map_err(|_| invalid("invalid administrative invocation"))?;
        let ctl = self.ctl;
        let tag = self.wire()?.write(ctl, 0, &bytes).map_err(wire_error)?;
        self.last_request = request;
        self.invocation_pending = true;
        let result = self.await_invocation(
            tag,
            bytes.len() as u32,
            request,
            catalog.generation,
            &command,
        );
        self.close_on_error(result)
    }

    fn await_invocation(
        &mut self,
        tag: Tag,
        length: u32,
        request: u64,
        generation: u64,
        command: &ControlCommand,
    ) -> io::Result<(u64, ControlOutcome, String)> {
        let frame = Duration::from_millis(self.info.frame_timeout_ms.into());
        let deadline =
            Instant::now() + Duration::from_millis(self.info.command_timeout_ms.into()) + frame;
        match self
            .wire()?
            .wait(tag, Instant::now() + frame)
            .map_err(wire_error)?
        {
            Reply::Write(n) if n == length => {}
            Reply::Error(errno) => {
                // A complete Rlerror proves this write was refused before acceptance.
                self.invocation_pending = false;
                return Err(io::Error::from_raw_os_error(errno.0 as i32));
            }
            _ => return Err(invalid("incomplete administrative write")),
        }
        loop {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "administrative result deadline",
                ));
            }
            let root = self.root;
            let bytes = snapshot(
                self.wire()?,
                root,
                b"status",
                ADMIN_LINE_MAX_BYTES,
                deadline.min(Instant::now() + frame),
            )?;
            match decode_admin_status(&bytes)
                .map_err(|_| invalid("invalid administrative status"))?
            {
                AdminStatus::Pending {
                    connection,
                    request: id,
                    generation: found,
                } if connection == self.info.connection && id == request && found == generation => {
                }
                AdminStatus::Outcome(outcome)
                    if outcome.connection == self.info.connection
                        && outcome.request == request
                        && outcome.generation == generation =>
                {
                    if outcome.outcome.success()
                        && !match command.owner {
                            ControlOwner::Policy => outcome.outcome == ControlOutcome::Committed,
                            ControlOwner::Session => {
                                outcome.outcome == ControlOutcome::Completed
                                    || (command.name == "reload-profile"
                                        && outcome.outcome == ControlOutcome::Unchanged)
                            }
                        }
                    {
                        return Err(invalid("incorrect administrative settlement"));
                    }
                    self.invocation_pending = false;
                    if command.owner == ControlOwner::Session
                        || matches!(
                            outcome.outcome,
                            ControlOutcome::Stale
                                | ControlOutcome::Denied
                                | ControlOutcome::Indeterminate
                        )
                    {
                        self.catalog = None;
                    }
                    return Ok((generation, outcome.outcome, outcome.detail));
                }
                _ => return Err(invalid("administrative result identity mismatch")),
            }
            // Status is an immutable snapshot, not a long-poll stream. Reopen
            // at a bounded cadence without extending the command's deadline.
            std::thread::sleep(
                Duration::from_millis(1).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }

    fn wire(&mut self) -> io::Result<&mut Pipeline> {
        self.wire.as_mut().ok_or_else(|| {
            invalid("administrative connection failed; mutation must not be replayed")
        })
    }
    fn close_on_error<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
        if result.is_err() {
            self.wire = None;
        }
        result
    }
}

fn open(
    wire: &mut Pipeline,
    root: Fid,
    name: &[u8],
    flags: u32,
    deadline: Instant,
) -> io::Result<Fid> {
    let (tag, fid) = wire.walk(root, &[name]).map_err(wire_error)?;
    if !matches!(wire.wait(tag, deadline).map_err(wire_error)?, Reply::Walk(qids) if qids.len() == 1)
    {
        return Err(invalid("administrative file walk refused"));
    }
    let tag = wire.lopen(fid, flags).map_err(wire_error)?;
    if !matches!(
        wire.wait(tag, deadline).map_err(wire_error)?,
        Reply::Lopen { .. }
    ) {
        return Err(invalid("administrative file open refused"));
    }
    Ok(fid)
}

fn snapshot(
    wire: &mut Pipeline,
    root: Fid,
    name: &[u8],
    maximum: usize,
    deadline: Instant,
) -> io::Result<Vec<u8>> {
    let fid = open(wire, root, name, 0, deadline)?;
    let mut bytes = Vec::new();
    loop {
        let count = (maximum + 1 - bytes.len()).min((wire.msize() - 11) as usize) as u32;
        let tag = wire
            .read(fid, bytes.len() as u64, count)
            .map_err(wire_error)?;
        let Reply::Read(chunk) = wire.wait(tag, deadline).map_err(wire_error)? else {
            return Err(invalid("administrative snapshot read refused"));
        };
        if chunk.is_empty() {
            break;
        }
        bytes.extend_from_slice(&chunk);
        if bytes.len() > maximum {
            return Err(invalid("administrative snapshot exceeds its bound"));
        }
    }
    let tag = wire.clunk(fid).map_err(wire_error)?;
    if wire.wait(tag, deadline).map_err(wire_error)? != Reply::Clunk {
        return Err(invalid("administrative snapshot clunk refused"));
    }
    Ok(bytes)
}

fn invalid(text: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, text)
}
fn wire_error(error: PipelineError) -> io::Error {
    let kind = match error {
        PipelineError::Io(kind) => kind,
        PipelineError::Timeout => io::ErrorKind::TimedOut,
        _ => io::ErrorKind::InvalidData,
    };
    io::Error::new(kind, error)
}
