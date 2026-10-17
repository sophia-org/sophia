//! One watch beside a snapshot, capture request or frame read.
//! No automatic recovery or replay.
use crate::{
    capture::Custody,
    capture_request::Exchange,
    frame::Exchange as FrameExchange,
    protocol::{CaptureRequest, CaptureState, FrameInfo, RequestCapability},
    snapshot::{Fetch, File, Operation},
    startup::Opened,
};
use sophia_9p_client::pipeline::{IoBudget, IoInterest, PipelineError, Reply};
use sophia_9p_records::{Fid, QidKind, Tag};
use std::os::fd::{AsFd, BorrowedFd};
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Busy,
    ClockRegression,
    Deadline,
    Closed,
    Wire(PipelineError),
    Snapshot(crate::snapshot::Error),
    Capture(crate::capture_request::Error),
    Frame(crate::frame::Error),
}
impl From<PipelineError> for Error {
    fn from(e: PipelineError) -> Self {
        Self::Wire(e)
    }
}
impl From<crate::snapshot::Error> for Error {
    fn from(e: crate::snapshot::Error) -> Self {
        Self::Snapshot(e)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchEvent {
    Revoked,
    Cancelled,
    Refused(u32),
}
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Walk,
    Open,
    Read,
    Clunk,
    Ready,
}
struct Watch {
    stage: Stage,
    fid: Fid,
    tag: Option<Tag>,
    flush: Option<Tag>,
    cancel: bool,
    event: Option<WatchEvent>,
    deadline: u64,
}
pub struct Session<'a> {
    opened: Option<Opened>,
    watch: Option<Watch>,
    fetch: Option<Fetch<'a>>,
    capture: Option<Exchange>,
    frame: Option<FrameExchange>,
    fetch_tag: Option<Tag>,
    file: Option<File>,
    now: u64,
    error: Option<Error>,
}
impl<'a> Session<'a> {
    /// Takes exclusive authenticated root ownership; retain the relay separately.
    /// Four request slots reserve room for watch, snapshot and flush traffic.
    pub fn new(opened: Opened) -> Result<Self, Error> {
        let w = &opened.pipeline;
        if !w.is_quiescent()
            || w.fid_count() != 1
            || !w.owns_fid(opened.root)
            || !(512..=65536).contains(&w.msize())
            || w.limits().max_fids < 3
            || w.limits().max_outstanding < 4
            || w.limits().max_buffered_input < w.msize() as usize * 2
        {
            return Err(Error::Invalid);
        }
        let now = opened.now;
        Ok(Self {
            opened: Some(opened),
            watch: None,
            fetch: None,
            capture: None,
            frame: None,
            fetch_tag: None,
            file: None,
            now,
            error: None,
        })
    }
    fn live(&self) -> Result<(), Error> {
        self.error.clone().map_or(Ok(()), Err)
    }
    fn fail(&mut self, e: Error) -> Error {
        if let Some(c) = &mut self.capture {
            c.fail(crate::capture_request::Error::Closed);
        }
        if let Some(f) = &mut self.frame {
            f.fail(crate::frame::Error::Closed);
        }
        self.opened.take();
        self.error.get_or_insert(e).clone()
    }
    pub fn close(&mut self) {
        self.fail(Error::Closed);
    }
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.opened.as_ref().map(|o| o.pipeline.as_fd())
    }
    pub fn io_interest(&self) -> Result<IoInterest, Error> {
        let mut interest = self
            .opened
            .as_ref()
            .ok_or(Error::Closed)?
            .pipeline
            .io_interest()?;
        interest.immediate |= self
            .watch
            .as_ref()
            .is_some_and(|w| w.cancel && w.stage == Stage::Read && w.flush.is_none());
        Ok(interest)
    }
    /// Next absolute wake time, including paced capture-status observation.
    /// Dispatch when due even if the descriptor is idle.
    pub fn deadline(&self) -> Option<u64> {
        if self.error.is_some() {
            return None;
        }
        self.watch
            .as_ref()
            .filter(|w| w.stage != Stage::Ready)
            .map(|w| w.deadline)
            .into_iter()
            .chain(self.fetch.as_ref().and_then(Fetch::deadline))
            .chain(self.capture.as_ref().and_then(Exchange::next_wake))
            .chain(self.frame.as_ref().and_then(FrameExchange::deadline))
            .min()
    }
    pub fn watch(&mut self, deadline: u64) -> Result<(), Error> {
        self.live()?;
        if self.watch.is_some() {
            return Err(Error::Busy);
        }
        if deadline <= self.now {
            return Err(Error::Deadline);
        }
        self.watch = Some(Watch {
            stage: Stage::Walk,
            fid: Fid::NOFID,
            tag: None,
            flush: None,
            cancel: false,
            event: None,
            deadline,
        });
        self.drive().map_err(|e| self.fail(e))
    }
    pub fn watch_waiting(&self) -> bool {
        self.error.is_none()
            && self
                .watch
                .as_ref()
                .is_some_and(|w| w.stage == Stage::Read && !w.cancel)
    }
    pub fn watch_event(&self) -> Option<WatchEvent> {
        if self.error.is_some() {
            return None;
        }
        self.watch
            .as_ref()
            .filter(|w| w.stage == Stage::Ready)
            .and_then(|w| w.event)
    }
    pub fn release_watch(&mut self) -> Result<(), Error> {
        self.live()?;
        if self.watch_event().is_none() {
            return Err(Error::Busy);
        }
        self.watch = None;
        Ok(())
    }
    /// Flushes a pending read and clunks its fid, preserving foreground work.
    /// A revocation reply that beats Rflush wins over local cancellation.
    pub fn cancel_watch(&mut self) -> Result<(), Error> {
        self.live()?;
        let watch = self.watch.as_mut().ok_or(Error::Invalid)?;
        if watch.stage != Stage::Ready {
            watch.cancel = true;
        }
        self.drive().map_err(|e| self.fail(e))
    }
    pub fn snapshot(&mut self, file: File, deadline: u64) -> Result<(), Error> {
        self.live()?;
        if self.fetch.is_some() || self.capture.is_some() || self.frame.is_some() {
            return Err(Error::Busy);
        }
        self.fetch = Some(Fetch::new(file, self.now, deadline)?);
        self.file = Some(file);
        self.drive().map_err(|e| self.fail(e))
    }
    pub fn transfer(&mut self, storage: &'a mut [u8], deadline: u64) -> Result<(), Error> {
        self.live()?;
        if self.fetch.is_some() || self.capture.is_some() || self.frame.is_some() {
            return Err(Error::Busy);
        }
        self.fetch = Some(Fetch::transfer(storage, self.now, deadline)?);
        self.file = Some(File::Transfer);
        self.drive().map_err(|e| self.fail(e))
    }
    pub fn snapshot_result(&self) -> Option<&[u8]> {
        if self.error.is_some() {
            None
        } else {
            self.fetch.as_ref().and_then(Fetch::result)
        }
    }
    /// One foreground capture request beside the retained watch. Reserve a
    /// serial above the recipient's durable high-water mark before calling;
    /// rejection or connection loss never permits replay. Storage is inline.
    pub fn capture(
        &mut self,
        request: CaptureRequest,
        previous_serial: u64,
        deadline: u64,
    ) -> Result<(), Error> {
        self.live()?;
        if self.fetch.is_some() || self.capture.is_some() || self.frame.is_some() {
            return Err(Error::Busy);
        }
        if self.opened.as_ref().ok_or(Error::Closed)?.info.request
            != RequestCapability::OutputCaptureV1
        {
            return Err(Error::Capture(crate::capture_request::Error::Unsupported));
        }
        self.capture = Some(
            Exchange::new(request, previous_serial, self.now, deadline).map_err(Error::Capture)?,
        );
        self.drive().map_err(|e| self.fail(e))
    }
    /// Custody remains inspectable after role failure; it does not grant pixels.
    pub fn capture_custody(&self) -> Option<Custody> {
        self.capture.as_ref().map(Exchange::custody)
    }
    /// Retained historical status and reserved serial, never a frame grant.
    pub fn capture_result(&self) -> Option<(CaptureState, u64)> {
        if self.error.is_some() {
            return None;
        }
        let c = self.capture.as_ref()?;
        Some((c.outcome()?, c.serial()))
    }
    pub fn release_capture(&mut self) -> Result<(), Error> {
        self.live()?;
        if self.capture_result().is_none() {
            return Err(Error::Busy);
        }
        self.capture = None;
        Ok(())
    }
    /// Retain metadata and pixels beside a watch. Six requests reserve four
    /// pixel reads plus watch/flush; four fids keep root, watch and both objects.
    /// Reuses the Vec, growing at most once after validated metadata.
    pub fn frame(&mut self, storage: Vec<u8>, deadline: u64) -> Result<(), Error> {
        self.live()?;
        if self.fetch.is_some() || self.capture.is_some() || self.frame.is_some() {
            return Err(Error::Busy);
        }
        let o = self.opened.as_ref().ok_or(Error::Closed)?;
        let w = &o.pipeline;
        // Four bounded Rreads plus the watch/error and flush replies. The
        // existing 60 KiB payload cap keeps default pipeline storage sufficient.
        let input = 4 * w.msize().min(crate::frame::READ_CHUNK + 11) as usize + 22;
        if w.limits().max_fids < 4
            || w.limits().max_outstanding < 6
            || w.limits().max_buffered_input < input
        {
            return Err(Error::Invalid);
        }
        self.frame =
            Some(FrameExchange::new(storage, w.msize(), self.now, deadline).map_err(Error::Frame)?);
        self.drive().map_err(|e| self.fail(e))
    }
    /// Received scratch bytes do not establish image validity.
    pub fn frame_received(&self) -> u64 {
        self.frame.as_ref().map_or(0, FrameExchange::received)
    }
    /// Exposed only after exact payload, EOF, metadata recheck and both clunks.
    pub fn frame_result(&self) -> Option<(FrameInfo, &[u8])> {
        if self.error.is_some() {
            return None;
        }
        self.frame.as_ref()?.result()
    }
    /// Release the foreground slot and move the completed buffer without copying.
    pub fn take_frame(&mut self) -> Result<(FrameInfo, Vec<u8>), Error> {
        self.live()?;
        if self.frame_result().is_none() {
            return Err(Error::Busy);
        }
        self.frame
            .take()
            .ok_or(Error::Invalid)?
            .finish()
            .map_err(Error::Frame)
    }
    /// Recover storage only after terminal role failure or explicit close.
    /// Bytes remain scratch; no validity claim survives failure.
    pub fn take_frame_scratch(&mut self) -> Result<Vec<u8>, Error> {
        if self.error.is_none() {
            return Err(Error::Busy);
        }
        Ok(self.frame.take().ok_or(Error::Invalid)?.into_scratch())
    }
    pub fn release_snapshot(&mut self) -> Result<(), Error> {
        self.live()?;
        if self.snapshot_result().is_none() || self.file == Some(File::Transfer) {
            return Err(Error::Busy);
        }
        self.fetch = None;
        self.file = None;
        Ok(())
    }
    pub fn take_transfer(&mut self) -> Result<(&'a mut [u8], usize), Error> {
        self.live()?;
        if self.snapshot_result().is_none() || self.file != Some(File::Transfer) {
            return Err(Error::Busy);
        }
        let result = self
            .fetch
            .take()
            .ok_or(Error::Invalid)?
            .into_transfer_storage()?;
        self.file = None;
        Ok(result)
    }
    pub fn finish(mut self) -> Result<Opened, Error> {
        self.live()?;
        if self.watch.is_some()
            || self.fetch.is_some()
            || self.capture.is_some()
            || self.frame.is_some()
        {
            return Err(Error::Busy);
        }
        let mut o = self.opened.take().ok_or(Error::Closed)?;
        o.now = self.now;
        Ok(o)
    }
    /// Shared byte/syscall budget and at most 32 reply rounds. No internal wait.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        self.live()?;
        self.now = now;
        if self
            .watch
            .as_ref()
            .is_some_and(|w| w.stage != Stage::Ready && now >= w.deadline)
            || self
                .fetch
                .as_ref()
                .and_then(Fetch::deadline)
                .is_some_and(|d| now >= d)
        {
            return Err(self.fail(Error::Deadline));
        }
        if let Err(e) = self.drive() {
            return Err(self.fail(e));
        }
        let mut budget = IoBudget::new(bytes);
        for _ in 0..32 {
            let w = &mut self.opened.as_mut().ok_or(Error::Closed)?.pipeline;
            if let Err(e) = w.poll_with_budget(&mut budget) {
                return Err(self.fail(e.into()));
            }
            let Some((tag, reply)) = w.take_reply() else {
                // A reply may already be decoded when cancellation is requested.
                // Drain it before flushing: settled tags cannot be flushed.
                if let Some(v) = &mut self.watch
                    && v.cancel
                    && v.stage == Stage::Read
                    && v.flush.is_none()
                    && let Some(tag) = v.tag
                {
                    match w.flush(tag) {
                        Ok(t) => v.flush = Some(t),
                        Err(e) => return Err(self.fail(e.into())),
                    }
                }
                break;
            };
            if let Err(e) = self.accept(tag, reply).and_then(|()| self.drive()) {
                return Err(self.fail(e));
            }
            if budget.exhausted() {
                break;
            }
        }
        Ok(())
    }
    fn drive(&mut self) -> Result<(), Error> {
        let o = self.opened.as_mut().ok_or(Error::Closed)?;
        let w = &mut o.pipeline;
        if let Some(f) = &mut self.frame {
            f.step(self.now, w, o.root).map_err(Error::Frame)?;
        }
        if let Some(c) = &mut self.capture {
            c.step(self.now, w, o.root).map_err(Error::Capture)?;
        }
        if let Some(v) = &mut self.watch
            && v.tag.is_none()
            && v.flush.is_none()
            && v.stage != Stage::Ready
        {
            v.tag = Some(match v.stage {
                Stage::Walk => {
                    let (t, f) = w.walk(o.root, &[b"watch"])?;
                    v.fid = f;
                    t
                }
                Stage::Open => w.lopen(v.fid, 0)?,
                Stage::Read => w.read(v.fid, 0, 1)?,
                Stage::Clunk => w.clunk(v.fid)?,
                Stage::Ready => unreachable!(),
            });
        }
        if self.fetch_tag.is_none()
            && let Some(fetch) = &mut self.fetch
            && let Some(op) = fetch.operation(self.now, w.msize())?
        {
            let mut fid = Fid::NOFID;
            let tag = match op {
                Operation::Walk(file) => {
                    let (t, f) = w.walk(o.root, &[file.name()])?;
                    fid = f;
                    t
                }
                Operation::Open(f) => w.lopen(f, 0)?,
                Operation::Read { fid, offset, count } => w.read(fid, offset, count)?,
                Operation::Clunk(f) => w.clunk(f)?,
            };
            fetch.issued(self.now, w.msize(), fid)?;
            self.fetch_tag = Some(tag);
        }
        Ok(())
    }
    fn accept(&mut self, tag: Tag, reply: Reply) -> Result<(), Error> {
        if let Some(f) = &mut self.frame
            && f.matches(tag)
        {
            return f.reply(tag, reply).map_err(Error::Frame);
        }
        if let Some(c) = &mut self.capture
            && c.matches(tag)
        {
            return c.reply(tag, reply).map_err(Error::Capture);
        }
        if self.fetch_tag == Some(tag) {
            self.fetch_tag = None;
            self.fetch
                .as_mut()
                .ok_or(Error::Invalid)?
                .reply(reply, self.now)?;
            return Ok(());
        }
        let v = self.watch.as_mut().ok_or(Error::Invalid)?;
        if v.flush == Some(tag) {
            if !matches!(reply, Reply::Flush) {
                return Err(Error::Invalid);
            }
            v.flush = None;
            v.tag = None;
            v.event.get_or_insert(WatchEvent::Cancelled);
            v.stage = Stage::Clunk;
            return Ok(());
        }
        if v.tag.take() != Some(tag) {
            return Err(Error::Invalid);
        }
        if let Reply::Error(e) = reply {
            v.event = Some(if e.0 == 116 && v.stage != Stage::Walk {
                WatchEvent::Revoked
            } else {
                WatchEvent::Refused(e.0)
            });
            if v.stage == Stage::Clunk {
                return Err(Error::Invalid);
            }
            v.stage = if v.stage == Stage::Walk {
                Stage::Ready
            } else {
                Stage::Clunk
            };
            return Ok(());
        }
        match (v.stage, reply) {
            (Stage::Walk, Reply::Walk(q)) if q.len() == 1 && q[0].kind == QidKind::File => {
                v.stage = Stage::Open
            }
            (Stage::Open, Reply::Lopen { qid, .. }) if qid.kind == QidKind::File => {
                v.stage = Stage::Read
            }
            (Stage::Clunk, Reply::Clunk) => v.stage = Stage::Ready,
            _ => return Err(Error::Invalid),
        }
        if v.cancel && matches!(v.stage, Stage::Open | Stage::Read) {
            v.event = Some(WatchEvent::Cancelled);
            v.stage = Stage::Clunk;
        }
        Ok(())
    }
}
