use super::*;
use std::collections::VecDeque;
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};

#[derive(Clone)]
pub struct InspectionPublisher {
    pub(super) shared: Arc<Shared>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishOutcome {
    Published { sequence: u64 },
    Unchanged,
    Busy { loss_generation: u64 },
}

pub(super) struct Shared {
    /// Only clone or swap an Arc while this lock is held. Never format, copy
    /// payload bytes, allocate qids, or destroy a retired view under it.
    pub view: Mutex<Arc<Publication>>,
    pub writer: Mutex<()>,
    pub next_qid: AtomicU64,
    /// Odd means fence transition; even nonzero values identify admitted views.
    pub generation: AtomicU64,
    pub wm_epoch: AtomicU64,
    pub excluded: AtomicU32,
    pub loss: AtomicU64,
    pub stopped: AtomicBool,
    wake: UnixStream,
}

pub(super) struct Snapshot {
    pub qid: u64,
    pub bytes: Arc<[u8]>,
    pub record: InspectionSnapshotRecord,
}
pub(super) struct Event {
    pub offset: u64,
    pub bytes: Vec<u8>,
}
#[derive(Clone)]
pub(super) struct Publication {
    pub generation: u64,
    pub snapshot: Option<Arc<Snapshot>>,
    pub events: VecDeque<Arc<Event>>,
    pub sequence: u64,
    pub tail: u64,
    pub ring_bytes: usize,
}
impl Publication {
    pub fn floor(&self) -> u64 {
        self.events.front().map_or(self.tail, |e| e.offset)
    }
}

impl Shared {
    pub fn new(wake: UnixStream) -> Self {
        Self {
            view: Mutex::new(Arc::new(Publication {
                generation: 0,
                snapshot: None,
                events: VecDeque::new(),
                sequence: 0,
                tail: 0,
                ring_bytes: 0,
            })),
            writer: Mutex::new(()),
            next_qid: AtomicU64::new(1),
            generation: AtomicU64::new(0),
            wm_epoch: AtomicU64::new(0),
            excluded: AtomicU32::new(0),
            loss: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            wake,
        }
    }
    pub fn view(&self) -> Result<Arc<Publication>, InspectionError> {
        Ok(self
            .view
            .lock()
            .map_err(|_| InspectionError::Poisoned)?
            .clone())
    }
    pub fn qids(&self, count: u64) -> Result<u64, InspectionError> {
        self.next_qid
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
                old.checked_add(count)
            })
            .map_err(|_| InspectionError::Exhausted)
    }
    pub fn wake(&self) {
        let _ = (&self.wake).write(&[1]);
    }
    pub fn fence(&self, epoch: u64, excluded: Option<u32>) -> Result<(), InspectionError> {
        if self.stopped.load(Ordering::SeqCst) {
            return Err(InspectionError::Stopped);
        }
        let old = self.generation.load(Ordering::SeqCst);
        if old != 0
            && self.wm_epoch.load(Ordering::SeqCst) == epoch
            && self.excluded.load(Ordering::SeqCst) == excluded.unwrap_or(0)
        {
            return Ok(());
        }
        let Some(next) = old.checked_add(2) else {
            self.stopped.store(true, Ordering::SeqCst);
            self.wake();
            return Err(InspectionError::Exhausted);
        };
        self.generation.store(old + 1, Ordering::SeqCst);
        self.wm_epoch.store(epoch, Ordering::SeqCst);
        self.excluded.store(excluded.unwrap_or(0), Ordering::SeqCst);
        self.generation.store(next, Ordering::SeqCst);
        self.wake();
        Ok(())
    }
    pub fn loss(&self) -> Result<u64, InspectionError> {
        match self
            .loss
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
        {
            Ok(old) => {
                self.wake();
                Ok(old + 1)
            }
            Err(_) => {
                self.stopped.store(true, Ordering::SeqCst);
                self.wake();
                Err(InspectionError::Exhausted)
            }
        }
    }
    pub fn valid(&self, generation: u64) -> bool {
        generation != 0
            && generation.is_multiple_of(2)
            && !self.stopped.load(Ordering::SeqCst)
            && self.generation.load(Ordering::SeqCst) == generation
    }
}

impl InspectionPublisher {
    fn busy(&self) -> Result<PublishOutcome, InspectionError> {
        Ok(PublishOutcome::Busy {
            loss_generation: self.shared.loss()?,
        })
    }
    /// No client or publication-lock wait. Loss invalidates existing watches
    /// immediately; even an unchanged later snapshot must publish a new cursor.
    pub fn publish(
        &self,
        snapshot: InspectionSnapshot,
        event: Option<InspectionEvent>,
    ) -> Result<PublishOutcome, InspectionError> {
        let result = self.publish_inner(snapshot, event);
        if matches!(
            result,
            Err(InspectionError::Record(_)
                | InspectionError::Exhausted
                | InspectionError::Poisoned)
        ) {
            // A refused owner report is observation loss too, not just mutex
            // contention. Never let a malformed report vanish silently.
            self.shared.loss()?;
        }
        result
    }

    fn publish_inner(
        &self,
        snapshot: InspectionSnapshot,
        event: Option<InspectionEvent>,
    ) -> Result<PublishOutcome, InspectionError> {
        let generation = self.shared.generation.load(Ordering::SeqCst);
        if self.shared.stopped.load(Ordering::SeqCst) {
            return Err(InspectionError::Stopped);
        }
        if !self.shared.valid(generation)
            || snapshot.wm_epoch != self.shared.wm_epoch.load(Ordering::SeqCst)
        {
            return Err(InspectionError::Fenced);
        }
        let _writer = match self.shared.writer.try_lock() {
            Ok(writer) => writer,
            Err(std::sync::TryLockError::WouldBlock) => {
                return self.busy();
            }
            Err(std::sync::TryLockError::Poisoned(_)) => return Err(InspectionError::Poisoned),
        };
        let snapshot = sanitize_inspection_snapshot(snapshot)?;
        let old = match self.shared.view.try_lock() {
            Ok(view) => view.clone(),
            Err(std::sync::TryLockError::WouldBlock) => return self.busy(),
            Err(std::sync::TryLockError::Poisoned(_)) => return Err(InspectionError::Poisoned),
        };
        if old.generation > generation || !self.shared.valid(generation) {
            return Err(InspectionError::Fenced);
        }
        let loss_generation = self.shared.loss.load(Ordering::SeqCst);
        if event.is_none()
            && old.snapshot.as_ref().is_some_and(|old| {
                old.record.generation == generation
                    && old.record.loss_generation == loss_generation
                    && old.record.snapshot == snapshot
            })
        {
            if !self.shared.valid(generation) {
                return Err(InspectionError::Fenced);
            }
            if self.shared.loss.load(Ordering::SeqCst) != loss_generation {
                return Ok(PublishOutcome::Busy {
                    loss_generation: self.shared.loss.load(Ordering::SeqCst),
                });
            }
            return Ok(PublishOutcome::Unchanged);
        }
        let sequence = old
            .sequence
            .checked_add(1)
            .ok_or(InspectionError::Exhausted)?;
        let event = encode_inspection_event(&InspectionEventRecord {
            schema: INSPECTION_SCHEMA,
            generation,
            sequence,
            loss_generation,
            event: event.unwrap_or(InspectionEvent::SnapshotChanged),
        })?;
        // This is checked before the eviction loop, even though today's
        // payload-free enum records are much smaller than the ring.
        if event.len() > INSPECTION_MAX_RING_BYTES {
            return Err(InspectionError::Record(InspectionRecordError::Bounds));
        }
        let event_offset = old
            .tail
            .checked_add(event.len() as u64)
            .ok_or(InspectionError::Exhausted)?;
        let record = InspectionSnapshotRecord {
            schema: INSPECTION_SCHEMA,
            generation,
            sequence,
            event_offset,
            loss_generation,
            snapshot,
        };
        let bytes = encode_inspection_snapshot(&record)?;
        let qid = self.shared.qids(1)?;
        let mut store = old.as_ref().clone();
        if store.snapshot.as_ref().is_some_and(|s| {
            s.record.generation != generation || s.record.loss_generation != loss_generation
        }) {
            store.events.clear();
            store.ring_bytes = 0;
        }
        while store.events.len() >= INSPECTION_MAX_EVENTS
            || store.ring_bytes + event.len() > INSPECTION_MAX_RING_BYTES
        {
            if let Some(old) = store.events.pop_front() {
                store.ring_bytes -= old.bytes.len();
            }
        }
        let offset = store.tail;
        store.ring_bytes += event.len();
        store.events.push_back(Arc::new(Event {
            offset,
            bytes: event,
        }));
        store.generation = generation;
        store.tail = event_offset;
        store.sequence = sequence;
        store.snapshot = Some(Arc::new(Snapshot {
            qid,
            bytes: bytes.into(),
            record,
        }));
        let outcome = self.handoff(&old, Arc::new(store), generation, loss_generation)?;
        if matches!(outcome, PublishOutcome::Published { .. }) {
            self.shared.wake();
        }
        Ok(outcome)
    }

    /// The only publication swap. Construction and retirement are both outside
    /// the handoff lock; a stale producer cannot replace a newer generation.
    pub(super) fn handoff(
        &self,
        old: &Arc<Publication>,
        next: Arc<Publication>,
        generation: u64,
        loss: u64,
    ) -> Result<PublishOutcome, InspectionError> {
        let mut view = match self.shared.view.try_lock() {
            Ok(view) => view,
            Err(std::sync::TryLockError::WouldBlock) => return self.busy(),
            Err(std::sync::TryLockError::Poisoned(_)) => return Err(InspectionError::Poisoned),
        };
        if !self.shared.valid(generation)
            || view.generation > generation
            || !Arc::ptr_eq(&view, old)
        {
            drop(view);
            return Err(InspectionError::Fenced);
        }
        if self.shared.loss.load(Ordering::SeqCst) != loss {
            drop(view);
            return Ok(PublishOutcome::Busy {
                loss_generation: self.shared.loss.load(Ordering::SeqCst),
            });
        }
        let sequence = next.sequence;
        let retired = std::mem::replace(&mut *view, next);
        drop(view);
        drop(retired);
        Ok(PublishOutcome::Published { sequence })
    }
}
