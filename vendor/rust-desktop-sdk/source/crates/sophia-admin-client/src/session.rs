//! Caller-driven administrative command session; one command and one snapshot.
use crate::{
    command::{Custody, Pending},
    protocol::{Command, Info, Outcome, Owner, decode_catalog},
    snapshot::{Fetch, File, Operation},
    startup::Opened,
};
use sophia_9p_client::pipeline::{IoBudget, IoInterest, Pipeline, PipelineError, Reply};
use sophia_9p_records::{Fid, Tag};
use std::os::fd::{AsFd, BorrowedFd};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Wire(PipelineError),
    Snapshot(crate::snapshot::Error),
    Command(crate::command::Error),
    Invalid,
    Busy,
    MissingCatalog,
    Unadvertised,
    Exhausted,
    ClockRegression,
    Closed,
}
impl From<PipelineError> for Error {
    fn from(v: PipelineError) -> Self {
        Self::Wire(v)
    }
}
impl From<crate::snapshot::Error> for Error {
    fn from(v: crate::snapshot::Error) -> Self {
        Self::Snapshot(v)
    }
}
impl From<crate::command::Error> for Error {
    fn from(v: crate::command::Error) -> Self {
        Self::Command(v)
    }
}
#[derive(Clone, Copy)]
struct Row {
    owner: Owner,
    start: usize,
    length: usize,
}
pub struct Session {
    wire: Option<Pipeline>,
    root: Fid,
    ctl: Fid,
    info: Info,
    now: u64,
    pending: Option<Tag>,
    fetch: Option<Fetch>,
    fetching: File,
    catalog: Vec<u8>,
    rows: [Row; 259],
    count: usize,
    generation: u64,
    valid: bool,
    scratch: Vec<u8>,
    command: Option<Pending>,
    next_request: u64,
    error: Option<Error>,
}
impl Session {
    pub fn from_opened(opened: Opened) -> Result<Self, Error> {
        if !opened.pipeline.is_quiescent()
            || opened.pipeline.fid_count() != 2
            || !opened.pipeline.owns_fid(opened.root)
            || !opened.pipeline.owns_fid(opened.ctl)
            || opened.root == opened.ctl
        {
            return Err(Error::Invalid);
        }
        let mut s = Self {
            wire: Some(opened.pipeline),
            root: opened.root,
            ctl: opened.ctl,
            info: opened.info,
            now: opened.now,
            pending: None,
            fetch: None,
            fetching: File::Commands,
            catalog: opened.scratch,
            rows: [Row {
                owner: Owner::Policy,
                start: 0,
                length: 0,
            }; 259],
            count: 0,
            generation: 0,
            valid: false,
            scratch: Vec::with_capacity(512),
            command: None,
            next_request: 1,
            error: None,
        };
        s.refresh_catalog(s.now)?;
        Ok(s)
    }
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }
    pub fn info(&self) -> Info {
        self.info
    }
    pub fn catalog_generation(&self) -> Option<u64> {
        self.valid.then_some(self.generation)
    }
    pub fn commands(&self) -> impl Iterator<Item = Command<'_>> {
        self.rows[..if self.valid { self.count } else { 0 }]
            .iter()
            .map(|r| Command {
                owner: r.owner,
                name: std::str::from_utf8(&self.catalog[r.start..r.start + r.length])
                    .expect("validated catalog"),
            })
    }
    fn contains(&self, command: Command<'_>) -> bool {
        self.rows[..self.count]
            .binary_search_by(|r| {
                let name = std::str::from_utf8(&self.catalog[r.start..r.start + r.length])
                    .expect("validated catalog");
                (r.owner, name).cmp(&(command.owner, command.name))
            })
            .is_ok()
    }
    pub fn latest_custody(&self) -> Option<(u64, Custody)> {
        self.command.as_ref().map(|p| (p.request(), p.custody()))
    }
    pub fn outcome(&self) -> Option<(Outcome, &str)> {
        self.command.as_ref().and_then(Pending::outcome)
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.wire.as_ref().map(AsFd::as_fd)
    }
    pub fn io_interest(&self) -> Result<IoInterest, Error> {
        self.wire
            .as_ref()
            .ok_or(Error::Closed)?
            .io_interest()
            .map_err(Into::into)
    }
    pub fn next_wake(&self) -> Option<u64> {
        if self.error.is_some() {
            return None;
        }
        self.fetch
            .as_ref()
            .and_then(Fetch::deadline)
            .into_iter()
            .chain(self.command.as_ref().and_then(Pending::next_wake))
            .min()
    }
    fn active_command(&self) -> bool {
        self.command.as_ref().is_some_and(|p| {
            matches!(
                p.custody(),
                Custody::Queued | Custody::Issued | Custody::Accepted
            )
        })
    }
    fn clock(&mut self, now: u64) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.now = now;
        Ok(())
    }
    fn fail(&mut self, error: Error) -> Error {
        let error = self.error.get_or_insert(error).clone();
        self.wire.take();
        if let Some(command) = &mut self.command {
            command.close();
        }
        self.fetch.take();
        self.pending = None;
        error
    }
    pub fn close(&mut self) {
        self.fail(Error::Closed);
    }
    pub fn refresh_catalog(&mut self, now: u64) -> Result<(), Error> {
        self.clock(now)?;
        if self.fetch.is_some() || self.pending.is_some() || self.active_command() {
            return Err(Error::Busy);
        }
        let deadline = now
            .checked_add(u64::from(self.info.frame_ms))
            .ok_or(Error::Invalid)?;
        self.valid = false;
        self.fetching = File::Commands;
        self.fetch = Some(Fetch::new(
            File::Commands,
            std::mem::take(&mut self.catalog),
            now,
            deadline,
        )?);
        if let Err(e) = self.drive() {
            return Err(self.fail(e));
        }
        Ok(())
    }
    pub fn invoke(&mut self, command: Command<'_>, now: u64) -> Result<u64, Error> {
        self.clock(now)?;
        if self.fetch.is_some() || self.pending.is_some() || self.active_command() {
            return Err(Error::Busy);
        }
        if !self.valid {
            return Err(Error::MissingCatalog);
        }
        if !self.contains(command) {
            return Err(Error::Unadvertised);
        }
        let request = self.next_request;
        let next = request.checked_add(1).ok_or(Error::Exhausted)?;
        let pending = Pending::selected(self.info, self.generation, command, request, now)?;
        self.command = Some(pending);
        self.next_request = next;
        if let Err(e) = self.drive() {
            return Err(self.fail(e));
        }
        Ok(request)
    }
    fn drive(&mut self) -> Result<(), Error> {
        if self.pending.is_some() {
            return Ok(());
        }
        let wire = self.wire.as_mut().ok_or(Error::Closed)?;
        if self.fetch.is_none()
            && let Some(command) = &mut self.command
        {
            command.advance(self.now)?;
            if command.custody() == Custody::Queued {
                self.pending = Some(wire.write(self.ctl, 0, command.bytes())?);
                command.issued(self.now)?;
                return Ok(());
            }
            if command.custody() == Custody::Accepted
                && command.next_wake().is_some_and(|t| self.now >= t)
            {
                let deadline = command.start_status(self.now)?;
                self.fetching = File::Status;
                self.fetch = Some(Fetch::new(
                    File::Status,
                    std::mem::take(&mut self.scratch),
                    self.now,
                    deadline,
                )?);
            }
        }
        if let Some(fetch) = &mut self.fetch {
            let op = fetch
                .operation(self.now, wire.msize())?
                .ok_or(Error::Invalid)?;
            let mut walked = Fid::NOFID;
            let tag = match op {
                Operation::Walk(file) => {
                    let (tag, fid) = wire.walk(self.root, &[file.name()])?;
                    walked = fid;
                    tag
                }
                Operation::Open(fid) => wire.lopen(fid, 0)?,
                Operation::Read { fid, offset, count } => wire.read(fid, offset, count)?,
                Operation::Clunk(fid) => wire.clunk(fid)?,
            };
            fetch.issued(self.now, wire.msize(), walked)?;
            self.pending = Some(tag);
        }
        Ok(())
    }
    fn accept(&mut self, reply: Reply) -> Result<(), Error> {
        if let Some(fetch) = &mut self.fetch {
            fetch.reply(reply, self.now)?;
            if fetch.result().is_some() {
                let bytes = self.fetch.take().unwrap().into_storage();
                if self.fetching == File::Commands {
                    let catalog = decode_catalog(&bytes).map_err(|_| Error::Invalid)?;
                    self.count = catalog.commands().len();
                    self.generation = catalog.generation();
                    for (dst, src) in self.rows.iter_mut().zip(catalog.commands()) {
                        *dst = Row {
                            owner: src.owner,
                            start: src.name.as_ptr() as usize - bytes.as_ptr() as usize,
                            length: src.name.len(),
                        };
                    }
                    self.catalog = bytes;
                    self.valid = true;
                } else {
                    let command = self.command.as_mut().ok_or(Error::Invalid)?;
                    command.status(&bytes, self.now)?;
                    if command.invalidate_catalog() {
                        self.valid = false;
                    }
                    self.scratch = bytes;
                }
            }
        } else {
            let command = self.command.as_mut().ok_or(Error::Invalid)?;
            let value = match reply {
                Reply::Write(n) => Ok(n),
                Reply::Error(e) => Err(e.0),
                _ => return Err(Error::Invalid),
            };
            command.write_reply(value, self.now)?;
        }
        Ok(())
    }
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), Error> {
        self.clock(now)?;
        if let Some(command) = &mut self.command
            && let Err(e) = command.advance(now)
        {
            return Err(self.fail(e.into()));
        }
        if let Some(fetch) = &self.fetch
            && let Err(e) = fetch.operation(now, self.wire.as_ref().ok_or(Error::Closed)?.msize())
        {
            return Err(self.fail(e.into()));
        }
        if let Err(e) = self.drive() {
            return Err(self.fail(e));
        }
        let mut budget = IoBudget::new(bytes);
        for _ in 0..32 {
            let wire = self.wire.as_mut().ok_or(Error::Closed)?;
            if let Err(e) = wire.poll_with_budget(&mut budget) {
                return Err(self.fail(e.into()));
            }
            let Some((tag, reply)) = wire.take_reply() else {
                break;
            };
            if self.pending.take() != Some(tag) {
                return Err(self.fail(Error::Invalid));
            }
            if let Err(e) = self.accept(reply).and_then(|()| self.drive()) {
                return Err(self.fail(e));
            }
            if budget.exhausted() {
                break;
            }
        }
        Ok(())
    }
}
