//! Custody for one lock provider connection epoch (t034, t294): the event
//! journal, the submission watermark, negotiation, resources, and the
//! candidates and frame demands handed to Session.
//!
//! The provider only renders. Nothing it submits can enter, leave or delay
//! the lock: it reads the lock object Session publishes, asks for frames,
//! and offers images for the allocations that object grants. Session tells
//! custody what the secret did and when a frame may be drawn; it never
//! hands over what the secret holds.
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sophia_9p::journal::{Journal, JournalBounds, JournalPosition};
use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::lock_files::*;

mod export;
mod presentation;
mod resources;

pub use export::{LockFileExport, LockFileHandle, LockFileNode, LockFileQids};

use presentation::{CandidatePlan, Presentation};
use resources::{ResourcePlan, Resources};

/// Reasons of the contract's open vocabulary.
pub mod reason {
    pub const NONE: u16 = 0;
    pub const STALE_LOCK: u16 = 1;
    pub const STALE_ALLOCATION: u16 = 2;
    pub const UNKNOWN_RESOURCE: u16 = 3;
    pub const SIZE_MISMATCH: u16 = 4;
    pub const PERMIT: u16 = 5;
    pub const BUDGET: u16 = 6;
}

/// Every reserved credit holds the largest event it may become.
const CREDIT_BYTES: usize = LOCK_FILE_HEADER_BYTES + 56;

/// At most this many items wait for Session.
pub const LOCK_FILE_INBOUND: usize = 32;

/// What Session fixes for one connection epoch.
pub struct LockFileSettings {
    pub epoch: u64,
    pub limits: LockFileLimits,
    /// Chords Session keeps for itself; a negotiation naming one is refused.
    pub reserved_chords: Vec<LockChordRequest>,
    /// The lock object in force when the connection is admitted, and the
    /// qid the export serves it under.
    pub lock: LockObject,
    pub lock_qid: u64,
}

/// What custody hands Session, in submission order.
pub enum LockInbound {
    /// Negotiation completed; chord IDs are indices into `chords`.
    Negotiated {
        chords: Vec<LockChordRequest>,
    },
    /// A whole image the provider may now name in candidates.
    ResourceReady {
        resource: LockResourceId,
        width_px: u32,
        height_px: u32,
        pixels: Arc<[u8]>,
    },
    ResourceRetired(LockResourceId),
    /// A candidate that matched the current lock object, its allocation
    /// and its permit. Session reports its terminal outcome.
    Candidate {
        candidate: LockCandidate,
        pixels: Arc<[u8]>,
    },
    /// A standing request for a frame; Session answers with a permit.
    Demand(LockFrameDemand),
}

impl core::fmt::Debug for LockInbound {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Negotiated { chords } => write!(formatter, "Negotiated({} chords)", chords.len()),
            Self::ResourceReady { resource, .. } => {
                write!(formatter, "ResourceReady({resource:?})")
            }
            Self::ResourceRetired(resource) => write!(formatter, "ResourceRetired({resource:?})"),
            Self::Candidate { candidate, .. } => write!(formatter, "Candidate({candidate:?})"),
            Self::Demand(demand) => write!(formatter, "Demand({demand:?})"),
        }
    }
}

enum Negotiation {
    Awaiting,
    Negotiated(LockNegotiated),
    Refused,
}

struct Accepted {
    submission: u64,
    bytes: Vec<u8>,
}

pub struct LockFileCustody {
    epoch: u64,
    limits: LockFileLimits,
    reserved_chords: Vec<LockChordRequest>,
    journal: Journal,
    lock: LockObject,
    lock_generation: u64,
    lock_qid: u64,
    negotiation: Negotiation,
    accepted: Option<Accepted>,
    resources: Resources,
    presentation: Presentation,
    inbound: VecDeque<LockInbound>,
    ack_deadline: Option<Instant>,
    revoked: bool,
}

impl LockFileCustody {
    pub fn new(settings: LockFileSettings) -> Result<Self, Errno> {
        if settings.epoch == 0 {
            return Err(Errno::EINVAL);
        }
        settings.limits.encode().map_err(|_| Errno::EINVAL)?;
        settings.lock.encode().map_err(|_| Errno::EINVAL)?;
        Ok(Self {
            epoch: settings.epoch,
            limits: settings.limits,
            reserved_chords: settings.reserved_chords,
            journal: Journal::new(settings.epoch),
            lock: settings.lock,
            lock_generation: 1,
            lock_qid: settings.lock_qid,
            negotiation: Negotiation::Awaiting,
            accepted: None,
            resources: Resources::new(settings.limits),
            presentation: Presentation::default(),
            inbound: VecDeque::with_capacity(LOCK_FILE_INBOUND),
            ack_deadline: None,
            revoked: false,
        })
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn limits(&self) -> LockFileLimits {
        self.limits
    }

    /// The current lock object, its generation and its qid.
    pub fn lock(&self) -> (&LockObject, u64, u64) {
        (&self.lock, self.lock_generation, self.lock_qid)
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked
    }

    pub fn revoke(&mut self) {
        self.revoked = true;
        self.inbound.clear();
    }

    pub fn position(&self) -> JournalPosition {
        self.journal.position()
    }

    pub fn read(&self, offset: u64, count: u32) -> Result<ReadOutcome, Errno> {
        self.journal.read(offset, count)
    }

    pub fn take_inbound(&mut self) -> Option<LockInbound> {
        self.inbound.pop_front()
    }

    /// Drops expired permits, and revokes a provider that stopped
    /// acknowledging its events.
    pub fn expire(&mut self, now: Instant) {
        self.presentation.expire(now);
        if self.ack_deadline.is_some_and(|deadline| now >= deadline) {
            self.revoke();
        }
    }

    /// How long a waiter may sleep before something here expires.
    pub fn wait(&self, now: Instant, maximum: Duration) -> Duration {
        self.ack_deadline.map_or(maximum, |deadline| {
            maximum.min(deadline.saturating_duration_since(now))
        })
    }

    fn live(&self) -> Result<(), Errno> {
        if self.revoked {
            Err(Errno::ESTALE)
        } else {
            Ok(())
        }
    }

    fn negotiated(&self) -> Result<LockNegotiated, Errno> {
        match self.negotiation {
            Negotiation::Negotiated(negotiated) => Ok(negotiated),
            Negotiation::Awaiting | Negotiation::Refused => Err(Errno::EACCES),
        }
    }

    fn event(&self, kind: LockFileKind, body: &[u8], index: u64) -> Result<Vec<u8>, Errno> {
        let sequence = self
            .journal
            .next_sequence()
            .checked_add(index)
            .ok_or(Errno::ENOSPC)?;
        encode_lock_file_record(
            LockFileHeader {
                kind,
                connection_epoch: self.epoch,
                submission_id: 0,
                sequence,
            },
            body,
        )
        .map_err(|_| Errno::EINVAL)
    }

    /// Journals `events` atomically, leaving room for `credits` reserved
    /// events afterwards. Nothing is journaled on any error.
    fn journal_events(
        &mut self,
        events: &[(LockFileKind, Vec<u8>)],
        credits: usize,
    ) -> Result<u64, Errno> {
        let records = events
            .iter()
            .enumerate()
            .map(|(index, (kind, body))| self.event(*kind, body, index as u64))
            .collect::<Result<Vec<_>, _>>()?;
        let bounds = JournalBounds {
            records: (self.limits.journal_records as usize)
                .checked_sub(credits)
                .ok_or(Errno::EAGAIN)?,
            bytes: (self.limits.journal_bytes as usize)
                .checked_sub(credits * CREDIT_BYTES)
                .ok_or(Errno::EAGAIN)?,
        };
        let first = self.journal.next_sequence();
        self.journal.prepare_batch(records, bounds)?.commit();
        if self.ack_deadline.is_none() {
            self.ack_deadline = Some(
                Instant::now() + Duration::from_millis(self.limits.ack_progress_timeout_ms.into()),
            );
        }
        Ok(first)
    }

    fn receipt(
        &self,
        submission_id: u64,
        kind: LockFileKind,
    ) -> Result<(LockFileKind, Vec<u8>), Errno> {
        let body = LockSubmitted {
            submission_id,
            candidate_kind: kind,
        }
        .encode()
        .map_err(|_| Errno::EINVAL)?;
        Ok((LockFileKind::Submitted, body))
    }

    pub fn acknowledge(&mut self, ack: LockFileAck) -> Result<(), Errno> {
        if ack.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if self.journal.ack(ack.connection_epoch, ack.sequence)? {
            self.ack_deadline = None;
            if self.journal.position().records != 0 {
                self.ack_deadline = Some(
                    Instant::now()
                        + Duration::from_millis(self.limits.ack_progress_timeout_ms.into()),
                );
            }
            if matches!(self.negotiation, Negotiation::Refused)
                && self.journal.position().records == 0
            {
                self.revoke();
            }
        }
        Ok(())
    }

    /// Takes custody of one complete candidate record. An exact repeat of
    /// the last accepted submission succeeds and changes nothing.
    pub fn submit(&mut self, bytes: &[u8], now: Instant) -> Result<(), Errno> {
        self.live()?;
        self.expire(now);
        let record =
            decode_lock_file_record(bytes, LockFileClass::Candidate).map_err(|_| Errno::EINVAL)?;
        if record.header.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        let submission = record.header.submission_id;
        if let Some(accepted) = &self.accepted {
            if submission == accepted.submission {
                return if bytes == accepted.bytes {
                    Ok(())
                } else {
                    Err(Errno::EINVAL)
                };
            }
            if submission < accepted.submission {
                return Err(Errno::ESTALE);
            }
        }
        if self.inbound.len() >= LOCK_FILE_INBOUND {
            return Err(Errno::EAGAIN);
        }
        let kind = record.header.kind;
        let receipt = self.receipt(submission, kind)?;
        let body = record.body;
        match kind {
            LockFileKind::Negotiate => self.negotiate(receipt, body)?,
            LockFileKind::ResourceBegin
            | LockFileKind::ResourceEnd
            | LockFileKind::ResourceCancel
            | LockFileKind::ResourceRetire => {
                self.negotiated()?;
                self.resource_step(receipt, kind, body)?;
            }
            LockFileKind::Candidate => {
                self.negotiated()?;
                self.candidate(receipt, body, now)?;
            }
            LockFileKind::FrameDemand => {
                self.negotiated()?;
                self.demand(receipt, body)?;
            }
            _ => return Err(Errno::EINVAL),
        }
        self.accepted = Some(Accepted {
            submission,
            bytes: bytes.to_vec(),
        });
        Ok(())
    }

    fn negotiate(&mut self, receipt: (LockFileKind, Vec<u8>), body: &[u8]) -> Result<(), Errno> {
        if !matches!(self.negotiation, Negotiation::Awaiting) {
            return Err(Errno::EINVAL);
        }
        let request = LockNegotiate::decode(body).map_err(|_| Errno::EINVAL)?;
        let refusal = if !(request.minimum_revision..=request.maximum_revision).contains(&1) {
            Some(LockRefusal::UnsupportedRevision)
        } else if request.requested_capabilities & LOCK_FILE_CAPABILITY_PRESENT == 0 {
            Some(LockRefusal::PresentationRequired)
        } else if request.chords.len() > usize::from(self.limits.max_chords)
            || (!request.chords.is_empty()
                && request.requested_capabilities & LOCK_FILE_CAPABILITY_CHORDS == 0)
            || request.chords.iter().any(|chord| {
                !chord.has_non_shift_modifier() || self.reserved_chords.contains(chord)
            })
        {
            Some(LockRefusal::InvalidChord)
        } else {
            None
        };
        let Some(refusal) = refusal else {
            let negotiated = LockNegotiated {
                granted_chords: request.chords.len() as u16,
                granted_capabilities: request.requested_capabilities
                    & (LOCK_FILE_CAPABILITY_PRESENT | LOCK_FILE_CAPABILITY_CHORDS),
            };
            let body = negotiated.encode().map_err(|_| Errno::EINVAL)?;
            let publication = self.publication_body(self.lock_generation, self.lock_qid)?;
            self.journal_events(
                &[
                    receipt,
                    (LockFileKind::Negotiated, body),
                    (LockFileKind::ObjectPublished, publication),
                ],
                self.presentation.credits(),
            )?;
            self.negotiation = Negotiation::Negotiated(negotiated);
            self.inbound.push_back(LockInbound::Negotiated {
                chords: request.chords,
            });
            return Ok(());
        };
        self.journal_events(
            &[receipt, (LockFileKind::Refused, refusal.encode())],
            self.presentation.credits(),
        )?;
        self.negotiation = Negotiation::Refused;
        Ok(())
    }

    fn resource_step(
        &mut self,
        receipt: (LockFileKind, Vec<u8>),
        kind: LockFileKind,
        body: &[u8],
    ) -> Result<(), Errno> {
        let invalid = |_| Errno::EINVAL;
        let plan = match kind {
            LockFileKind::ResourceBegin => self
                .resources
                .plan_begin(LockResourceBegin::decode(body).map_err(invalid)?)?,
            LockFileKind::ResourceEnd => self
                .resources
                .plan_end(LockResourceStep::decode(body, true).map_err(invalid)?)?,
            LockFileKind::ResourceCancel => self
                .resources
                .plan_cancel(LockResourceStep::decode(body, false).map_err(invalid)?)?,
            _ => self
                .resources
                .plan_retire(LockResourceStep::decode(body, false).map_err(invalid)?)?,
        };
        let retired = match plan {
            ResourcePlan::Retire { resource, .. } => Some(resource),
            _ => None,
        };
        self.journal_events(&[receipt, plan.event()?], self.presentation.credits())?;
        if let Some((resource, image)) = self.resources.apply(plan) {
            self.inbound.push_back(LockInbound::ResourceReady {
                resource,
                width_px: image.width_px,
                height_px: image.height_px,
                pixels: image.pixels,
            });
        }
        if let Some(resource) = retired {
            self.inbound
                .push_back(LockInbound::ResourceRetired(resource));
        }
        Ok(())
    }

    fn candidate(
        &mut self,
        receipt: (LockFileKind, Vec<u8>),
        body: &[u8],
        now: Instant,
    ) -> Result<(), Errno> {
        let candidate = LockCandidate::decode(body).map_err(|_| Errno::EINVAL)?;
        let plan = self
            .presentation
            .plan_candidate(candidate, &self.lock, &self.resources, now)?;
        let pixels = match &plan {
            CandidatePlan::Forward(candidate) => Some(
                self.resources
                    .image(candidate.resource)
                    .map(|image| Arc::clone(&image.pixels))
                    .ok_or(Errno::EINVAL)?,
            ),
            CandidatePlan::Reject(_) => None,
        };
        let credits = self.presentation.credits();
        match &plan {
            CandidatePlan::Forward(_) => {
                self.journal_events(&[receipt], credits + 1)?;
            }
            CandidatePlan::Reject(outcome) => {
                let body = outcome.encode().map_err(|_| Errno::EINVAL)?;
                self.journal_events(&[receipt, (LockFileKind::CandidateOutcome, body)], credits)?;
            }
        }
        self.presentation.apply_candidate(&plan);
        if let (CandidatePlan::Forward(candidate), Some(pixels)) = (plan, pixels) {
            self.inbound
                .push_back(LockInbound::Candidate { candidate, pixels });
        }
        Ok(())
    }

    fn demand(&mut self, receipt: (LockFileKind, Vec<u8>), body: &[u8]) -> Result<(), Errno> {
        let demand = LockFrameDemand::decode(body).map_err(|_| Errno::EINVAL)?;
        let added = self.presentation.plan_demand(demand, &self.lock)?;
        self.journal_events(&[receipt], self.presentation.credits() + usize::from(added))?;
        self.presentation.apply_demand(demand);
        self.inbound.push_back(LockInbound::Demand(demand));
        Ok(())
    }

    /// Appends upload bytes for `binding` of `slot`.
    pub fn write_upload(
        &mut self,
        slot: u8,
        binding: u64,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        self.live()?;
        self.negotiated()?;
        self.resources.write(slot, binding, offset, data)
    }

    /// The binding `slot` holds: an upload fid opened now writes to it.
    pub fn upload_binding(&self, slot: u8) -> Option<u64> {
        self.resources.binding(slot)
    }

    fn publication_body(&self, generation: u64, qid: u64) -> Result<Vec<u8>, Errno> {
        LockObjectPublished {
            object_generation: generation,
            qid_path: qid,
        }
        .encode()
        .map_err(|_| Errno::EINVAL)
    }

    /// Session publishes a new lock object: on every phase, lock epoch or
    /// topology change. Forwarded candidates and demands for allocations it
    /// no longer grants lapse. Returns the new generation.
    pub fn publish_lock(&mut self, lock: LockObject, qid: u64) -> Result<u64, Errno> {
        self.live()?;
        lock.encode().map_err(|_| Errno::EINVAL)?;
        let generation = self.lock_generation.checked_add(1).ok_or(Errno::ENOSPC)?;
        let revoked = self.presentation.plan_publication(&lock);
        if matches!(self.negotiation, Negotiation::Negotiated(_)) {
            let mut events = Vec::with_capacity(revoked.len() + 1);
            for outcome in &revoked {
                events.push((
                    LockFileKind::CandidateOutcome,
                    outcome.encode().map_err(|_| Errno::EINVAL)?,
                ));
            }
            events.push((
                LockFileKind::ObjectPublished,
                self.publication_body(generation, qid)?,
            ));
            self.journal_events(&events, self.presentation.credits() - revoked.len())?;
        }
        self.presentation.apply_publication(&lock, &revoked);
        self.lock = lock;
        self.lock_generation = generation;
        self.lock_qid = qid;
        Ok(generation)
    }

    /// What the secret did. Refused before negotiation, and when the
    /// journal has no room, which only the provider's own lag explains.
    pub fn entry(&mut self, entry: LockEntry) -> Result<u64, Errno> {
        self.live()?;
        self.negotiated()?;
        let body = entry.encode().map_err(|_| Errno::EINVAL)?;
        self.journal_events(&[(LockFileKind::Entry, body)], self.presentation.credits())
    }

    /// A granted chord, by its ID.
    pub fn chord(&mut self, chord: LockChord) -> Result<u64, Errno> {
        self.live()?;
        let negotiated = self.negotiated()?;
        if negotiated.granted_capabilities & LOCK_FILE_CAPABILITY_CHORDS == 0
            || chord.chord >= negotiated.granted_chords
        {
            return Err(Errno::EINVAL);
        }
        let body = chord.encode().map_err(|_| Errno::EINVAL)?;
        self.journal_events(&[(LockFileKind::Chord, body)], self.presentation.credits())
    }

    /// Session's answer to a standing demand. Spends the demand's credit.
    pub fn permit(
        &mut self,
        allocation_id: u64,
        demand_id: u64,
        expires_after: Duration,
        now: Instant,
    ) -> Result<LockFramePermit, Errno> {
        self.live()?;
        let permit = self
            .presentation
            .plan_permit(allocation_id, demand_id, expires_after)?;
        let body = permit.encode().map_err(|_| Errno::EINVAL)?;
        self.journal_events(
            &[(LockFileKind::FramePermit, body)],
            self.presentation.credits() - 1,
        )?;
        self.presentation.apply_permit(permit, now);
        Ok(permit)
    }

    /// Session's outcome for a forwarded candidate.
    pub fn outcome(&mut self, outcome: LockCandidateOutcome) -> Result<u64, Errno> {
        self.live()?;
        let terminal = self.presentation.plan_outcome(&outcome)?;
        let body = outcome.encode().map_err(|_| Errno::EINVAL)?;
        let sequence = self.journal_events(
            &[(LockFileKind::CandidateOutcome, body)],
            self.presentation.credits() - usize::from(terminal),
        )?;
        self.presentation.apply_outcome(&outcome);
        Ok(sequence)
    }
}
