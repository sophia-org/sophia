//! Frame operation state. Queues requests but never polls or owns the role.
use super::{Error, READ_CHUNK};
use crate::{
    protocol::{FrameInfo, decode_frame_info},
    read_window::{ReadWindow, Request},
};
use sophia_9p_client::pipeline::{Pipeline, Reply};
use sophia_9p_records::{Fid, QidKind, Tag};
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    WalkInfo,
    OpenInfo,
    Info,
    WalkFrame,
    OpenFrame,
    Pixels,
    Eof,
    Recheck,
    ClunkFrame,
    ClunkInfo,
    Ready,
}
pub(crate) struct Exchange {
    msize: u32,
    stage: Stage,
    pending: Option<Tag>,
    info_fid: Fid,
    frame_fid: Fid,
    info_unit: u32,
    sent: usize,
    record: [u8; 64],
    check: [u8; 64],
    used: usize,
    info: Option<FrameInfo>,
    pixels: Vec<u8>,
    window: Option<ReadWindow>,
    reads: [Option<(Tag, Request)>; 4],
    now: u64,
    deadline: u64,
    error: Option<Error>,
}
impl Exchange {
    pub(crate) fn new(
        mut storage: Vec<u8>,
        msize: u32,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        Self::take(&mut storage, msize, now, deadline)
    }
    pub(crate) fn take(
        storage: &mut Vec<u8>,
        msize: u32,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if deadline <= now {
            return Err(Error::Deadline);
        }
        Ok(Self {
            msize,
            stage: Stage::WalkInfo,
            pending: None,
            info_fid: Fid::NOFID,
            frame_fid: Fid::NOFID,
            info_unit: 0,
            sent: 0,
            record: [0; 64],
            check: [0; 64],
            used: 0,
            info: None,
            pixels: std::mem::take(storage),
            window: None,
            reads: [None; 4],
            now,
            deadline,
            error: None,
        })
    }
    pub(crate) fn now(&self) -> u64 {
        self.now
    }
    pub(crate) fn matches(&self, tag: Tag) -> bool {
        self.pending == Some(tag) || self.reads.iter().flatten().any(|(t, _)| *t == tag)
    }
    pub fn deadline(&self) -> Option<u64> {
        (self.error.is_none() && self.stage != Stage::Ready).then_some(self.deadline)
    }
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }
    /// Accepted payload bytes, including incomplete scratch. Not image validity.
    pub fn received(&self) -> u64 {
        self.window.as_ref().map_or(0, ReadWindow::received)
    }
    pub fn result(&self) -> Option<(FrameInfo, &[u8])> {
        if self.error.is_none() && self.stage == Stage::Ready {
            Some((self.info?, &self.pixels))
        } else {
            None
        }
    }
    pub(crate) fn finish(self) -> Result<(FrameInfo, Vec<u8>), Error> {
        if let Some(e) = self.error {
            return Err(e);
        }
        if self.stage != Stage::Ready {
            return Err(Error::Invalid);
        }
        Ok((self.info.ok_or(Error::Invalid)?, self.pixels))
    }
    /// Failure/cancellation may leave scratch bytes, never a valid image.
    pub fn into_scratch(self) -> Vec<u8> {
        self.pixels
    }
    pub(crate) fn fail(&mut self, e: Error) -> Error {
        self.error.get_or_insert(e).clone()
    }
    pub(crate) fn step(&mut self, now: u64, w: &mut Pipeline, root: Fid) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.now = now;
        if self.stage == Stage::Ready {
            return Ok(());
        }
        if now >= self.deadline {
            return Err(Error::Deadline);
        }
        self.drive(w, root)
    }
    pub(crate) fn reply(&mut self, tag: Tag, reply: Reply) -> Result<(), Error> {
        if !self.matches(tag) {
            return Err(Error::Invalid);
        }
        self.accept(tag, reply)
    }
    fn drive(&mut self, w: &mut Pipeline, root: Fid) -> Result<(), Error> {
        if self.pending.is_some() || self.stage == Stage::Ready {
            return Ok(());
        }
        if self.stage == Stage::Pixels {
            let window = self.window.as_mut().ok_or(Error::Invalid)?;
            for slot in &mut self.reads {
                if slot.is_none()
                    && let Some(r) = window.reserve().map_err(|_| Error::Invalid)?
                {
                    *slot = Some((w.read(self.frame_fid, r.offset, r.count)?, r));
                }
            }
            return Ok(());
        }
        self.pending = Some(match self.stage {
            Stage::WalkInfo => {
                let (t, f) = w.walk(root, &[b"frame-info"])?;
                self.info_fid = f;
                t
            }
            Stage::WalkFrame => {
                let (t, f) = w.walk(root, &[b"frame"])?;
                self.frame_fid = f;
                t
            }
            Stage::OpenInfo => w.lopen(self.info_fid, 0)?,
            Stage::OpenFrame => w.lopen(self.frame_fid, 0)?,
            Stage::Info | Stage::Recheck => {
                let mut count = (65 - self.used).min((w.msize() - 11) as usize) as u32;
                if self.info_unit > 0 {
                    count = count.min(self.info_unit);
                }
                self.sent = count as usize;
                w.read(self.info_fid, self.used as u64, count)?
            }
            Stage::Eof => w.read(self.frame_fid, self.pixels.len() as u64, 1)?,
            Stage::ClunkFrame => w.clunk(self.frame_fid)?,
            Stage::ClunkInfo => w.clunk(self.info_fid)?,
            _ => return Err(Error::Invalid),
        });
        Ok(())
    }
    fn accept(&mut self, tag: Tag, reply: Reply) -> Result<(), Error> {
        if let Reply::Error(e) = reply {
            return Err(Error::Remote(e.0));
        }
        if self.stage == Stage::Pixels {
            let slot = self
                .reads
                .iter_mut()
                .find(|s| s.as_ref().is_some_and(|(t, _)| *t == tag))
                .ok_or(Error::Invalid)?;
            let (_, request) = slot.take().ok_or(Error::Invalid)?;
            let Reply::Read(data) = reply else {
                return Err(Error::Invalid);
            };
            let window = self.window.as_mut().ok_or(Error::Invalid)?;
            let offset = window
                .accept(request.token, data.len())
                .map_err(|_| Error::Invalid)? as usize;
            self.pixels[offset..offset + data.len()].copy_from_slice(&data);
            if window.payload_complete() {
                self.stage = Stage::Eof;
            }
            return Ok(());
        }
        if self.pending.take() != Some(tag) {
            return Err(Error::Invalid);
        }
        match (self.stage, reply) {
            (Stage::WalkInfo, Reply::Walk(q)) if q.len() == 1 && q[0].kind == QidKind::File => {
                self.stage = Stage::OpenInfo
            }
            (Stage::WalkFrame, Reply::Walk(q)) if q.len() == 1 && q[0].kind == QidKind::File => {
                self.stage = Stage::OpenFrame
            }
            (Stage::OpenInfo, Reply::Lopen { qid, iounit }) if qid.kind == QidKind::File => {
                self.info_unit = iounit;
                self.stage = Stage::Info;
            }
            (Stage::OpenFrame, Reply::Lopen { qid, iounit }) if qid.kind == QidKind::File => {
                self.window = Some(
                    ReadWindow::new(self.pixels.len() as u64, self.msize, iounit, READ_CHUNK)
                        .map_err(|_| Error::Invalid)?,
                );
                self.stage = Stage::Pixels;
            }
            (Stage::Info | Stage::Recheck, Reply::Read(data)) => {
                if data.len() > self.sent || data.len() > 64 - self.used {
                    return Err(Error::Invalid);
                }
                if data.is_empty() {
                    if self.used != 64 {
                        return Err(Error::Invalid);
                    }
                    if self.stage == Stage::Recheck {
                        if self.check != self.record {
                            return Err(Error::Invalid);
                        }
                        self.stage = Stage::ClunkFrame;
                    } else {
                        let info = decode_frame_info(&self.record).map_err(|_| Error::Invalid)?;
                        let length = info.image_bytes as usize;
                        self.pixels
                            .try_reserve(length.saturating_sub(self.pixels.len()))
                            .map_err(|_| Error::Capacity)?;
                        self.pixels.resize(length, 0);
                        self.info = Some(info);
                        self.stage = Stage::WalkFrame;
                    }
                } else {
                    let target = if self.stage == Stage::Info {
                        &mut self.record
                    } else {
                        &mut self.check
                    };
                    target[self.used..self.used + data.len()].copy_from_slice(&data);
                    self.used += data.len();
                }
            }
            (Stage::Eof, Reply::Read(data)) if data.is_empty() => {
                self.used = 0;
                self.stage = Stage::Recheck;
            }
            (Stage::ClunkFrame, Reply::Clunk) => self.stage = Stage::ClunkInfo,
            (Stage::ClunkInfo, Reply::Clunk) => self.stage = Stage::Ready,
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
}
