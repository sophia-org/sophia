use crate::session::{SessionError, busy};
use sophia_9p_client::pipeline::{Pipeline, Reply};
use sophia_9p_records::{Fid, QidKind, Tag};
use sophia_wm_protocol::wm_files::*;

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    Walk,
    Open,
    Read,
    Clunk,
    Ready,
}

pub(crate) struct Snapshot {
    stage: Stage,
    tag: Option<Tag>,
    fid: Fid,
    iounit: u32,
    epoch: u64,
    transaction: u64,
    scene: u64,
    capabilities: u64,
    bytes: Vec<u8>,
    value: Option<WmFileSnapshot>,
    deadline: Option<u64>,
    retry_at: Option<u64>,
    retry_delay: u64,
}
impl Snapshot {
    pub(crate) fn new() -> Self {
        Self {
            stage: Stage::Idle,
            tag: None,
            fid: Fid::NOFID,
            iounit: 0,
            epoch: 0,
            transaction: 0,
            scene: 0,
            capabilities: 0,
            bytes: vec![],
            value: None,
            deadline: None,
            retry_at: None,
            retry_delay: 0,
        }
    }
    pub(crate) fn request(
        &mut self,
        cycle: &WmFileCycle,
        capabilities: u64,
        now: u64,
        deadline: u64,
    ) -> Result<(), SessionError> {
        if self.stage != Stage::Idle {
            return Err(SessionError::SnapshotBusy);
        }
        if deadline <= now {
            return Err(SessionError::Deadline);
        }
        self.epoch = cycle.request.connection_epoch;
        self.transaction = cycle.snapshot_transaction.raw();
        self.scene = cycle.request.scene_generation;
        self.capabilities = capabilities;
        self.bytes.clear();
        self.value = None;
        self.retry_at = None;
        self.retry_delay = 0;
        self.deadline = Some(deadline);
        self.stage = Stage::Walk;
        Ok(())
    }
    pub(crate) fn deadline(&self) -> Option<u64> {
        self.deadline
    }
    pub(crate) fn pending(&self) -> bool {
        !matches!(self.stage, Stage::Idle | Stage::Ready)
    }
    pub(crate) fn retry_at(&self) -> Option<u64> {
        self.retry_at
    }
    pub(crate) fn tag(&self) -> Option<Tag> {
        self.tag
    }
    pub(crate) fn result(&self) -> Option<&WmFileSnapshot> {
        if self.stage == Stage::Ready {
            self.value.as_ref()
        } else {
            None
        }
    }
    pub(crate) fn release(&mut self) -> Result<(), SessionError> {
        if self.stage != Stage::Ready {
            return Err(SessionError::NotReady);
        }
        self.value = None;
        self.bytes.clear();
        self.stage = Stage::Idle;
        Ok(())
    }
    pub(crate) fn clear(&mut self) {
        *self = Self::new();
    }
    pub(crate) fn drive(
        &mut self,
        wire: &mut Pipeline,
        root: Fid,
        now: u64,
    ) -> Result<bool, SessionError> {
        if self.tag.is_some()
            || matches!(self.stage, Stage::Idle | Stage::Ready)
            || self.retry_at.is_some_and(|at| now < at)
        {
            return Ok(false);
        }
        if self.deadline.is_some_and(|at| now >= at) {
            return Err(SessionError::Deadline);
        }
        let result = match self.stage {
            Stage::Walk => wire.walk(root, &[b"snapshot"]).map(|(tag, fid)| {
                self.fid = fid;
                tag
            }),
            Stage::Open => wire.lopen(self.fid, 0),
            Stage::Read => {
                let mut n = (WM_FILE_MAX_BYTES - self.bytes.len())
                    .max(1)
                    .min((wire.msize() - 11) as usize) as u32;
                if self.iounit != 0 {
                    n = n.min(self.iounit);
                }
                wire.read(self.fid, self.bytes.len() as u64, n)
            }
            Stage::Clunk => wire.clunk(self.fid),
            _ => return Ok(false),
        };
        match result {
            Ok(tag) => {
                self.tag = Some(tag);
                Ok(true)
            }
            Err(error) if busy(&error) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
    pub(crate) fn reply(&mut self, reply: Reply, now: u64) -> Result<(), SessionError> {
        self.tag = None;
        if let Reply::Error(errno) = reply {
            if self.stage == Stage::Open && errno.0 == 11 {
                self.retry_delay = if self.retry_delay == 0 {
                    4
                } else {
                    (self.retry_delay * 2).min(256)
                };
                self.retry_at = Some(now.saturating_add(self.retry_delay));
                return Ok(());
            }
            return Err(SessionError::Protocol("snapshot request refused"));
        }
        match (self.stage, reply) {
            (Stage::Walk, Reply::Walk(qids))
                if qids.len() == 1 && qids[0].kind == QidKind::File =>
            {
                self.stage = Stage::Open
            }
            (Stage::Open, Reply::Lopen { qid, iounit }) if qid.kind == QidKind::File => {
                self.iounit = iounit;
                self.retry_at = None;
                self.stage = Stage::Read;
            }
            (Stage::Read, Reply::Read(data)) => {
                if data.len() > WM_FILE_MAX_BYTES - self.bytes.len() {
                    return Err(SessionError::Protocol("snapshot exceeds cap"));
                }
                if data.is_empty() {
                    let header = decode_wm_file_record(&self.bytes, WmFileClass::Object)
                        .map_err(|_| SessionError::Protocol("invalid snapshot envelope"))?
                        .header;
                    let snapshot = decode_wm_file_snapshot(&self.bytes, self.capabilities)
                        .map_err(|_| SessionError::Protocol("invalid snapshot body"))?;
                    if header.connection_epoch != self.epoch
                        || snapshot.transaction.raw() != self.transaction
                        || snapshot.snapshot.scene.generation != self.scene
                    {
                        return Err(SessionError::Protocol("snapshot does not match Cycle"));
                    }
                    self.value = Some(snapshot);
                    self.stage = Stage::Clunk;
                } else {
                    self.bytes.extend_from_slice(&data);
                    if self.bytes.len() >= 4 {
                        let declared =
                            u32::from_le_bytes(self.bytes[..4].try_into().unwrap()) as usize;
                        if !(WM_FILE_HEADER_BYTES..=WM_FILE_MAX_BYTES).contains(&declared)
                            || self.bytes.len() > declared
                        {
                            return Err(SessionError::Protocol("invalid snapshot length"));
                        }
                    }
                }
            }
            (Stage::Clunk, Reply::Clunk) => {
                self.stage = Stage::Ready;
                self.deadline = None;
                self.bytes.clear();
            }
            _ => return Err(SessionError::Protocol("unexpected snapshot reply")),
        }
        Ok(())
    }
}
