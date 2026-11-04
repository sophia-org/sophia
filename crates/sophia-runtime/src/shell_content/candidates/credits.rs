//! Stream store adapter; production SFT negotiation remains disabled.
use super::*;
use crate::frame_credits::{FrameCredits, FrameStreamClose, FrameStreamKey, FrameStreamPhase};

mod commit;
pub use commit::{ContentFrameAdmission, ContentFrameCommit};
pub(crate) use commit::{FrameAdmissionReservation, FrameCommitState};
mod control;
mod copy;
mod events;
pub(crate) use copy::{FrameCopyBinding, FrameCopyState};
mod intake;
pub(super) use events::CandidateEvents;
pub use intake::ContentFrameRequestAdmission;

/// Stream outcome identity and optional server-committed terminal replenishment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentFrameCreditReturn {
    pub key: FrameStreamKey,
    pub expires_msec: Option<u64>,
}

/// Local control records, ordered with ordinary candidate outcomes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentFrameStreamRecord {
    Opened {
        key: FrameStreamKey,
        credits: u8,
        ttl_ms: u32,
    },
    Closed(FrameStreamClose),
    /// Refusal of an input request, never a terminal for an accepted candidate.
    Refused {
        request_kind: sophia_protocol::shell_files::ShellFileKind,
        grant: ContentGrant,
        output: ContentOutputId,
        stream_id: u64,
        candidate_generation: u64,
        reason: ContentReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentFrameStreamEvent {
    pub transaction: TransactionId,
    pub record: ContentFrameStreamRecord,
}

pub(super) struct Entry {
    ledger: FrameCredits,
    notice_transaction: TransactionId,
}

#[derive(Default)]
pub(super) struct CreditStreams {
    pub(super) entries: BTreeMap<ContentOutputId, Entry>,
    last_stream_id: u64,
    poisoned: bool,
}

impl CreditStreams {
    pub(super) fn quiescent(&self) -> bool {
        self.entries
            .values()
            .all(|entry| entry.ledger.phase() == FrameStreamPhase::Closed)
    }
}

impl ContentCandidateStore {
    /// Open the native CPU profile with exactly one outstanding-frame credit.
    /// The owner supplies fresh context and must separately admit the wire reply.
    pub fn open_native_frame_stream(
        &mut self,
        transaction: TransactionId,
        key: FrameStreamKey,
        context: ContentCandidateContext<'_>,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        self.open_frame_stream(transaction, key, 1, context, now)
    }

    /// Open a stream after the owner has selected the server's credit allowance.
    /// The caller supplies current output facts and allocations, not peer claims.
    pub fn open_frame_stream(
        &mut self,
        transaction: TransactionId,
        key: FrameStreamKey,
        credits: u8,
        context: ContentCandidateContext<'_>,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        self.open_frame_stream_with_room(transaction, key, credits, context, now, usize::MAX)
    }

    fn open_frame_stream_with_room(
        &mut self,
        transaction: TransactionId,
        key: FrameStreamKey,
        credits: u8,
        context: ContentCandidateContext<'_>,
        now: u64,
        response_room: usize,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(key.grant)?;
        self.time(now)?;
        self.expire(now)?;
        self.prune_closed_streams();
        if self.profile != ContentStoreProfile::Legacy
            || !transaction.is_valid()
            || key.stream_id <= self.streams.last_stream_id
            || !Self::stream_context_matches(key, &context)
            || self.streams.entries.contains_key(&key.output)
            || self.demands.contains_key(&key.output)
            || self.permits.contains_key(&key.output)
            || self.assemblies.contains_key(&key.output)
            || self.pending.contains_key(&key.output)
            || self.submitted.contains_key(&key.output)
            || self.events.has_output(key.output)
        {
            return Err(ContentCandidateError::Stale);
        }
        if response_room < 2
            || self.open_candidate_slots() >= self.limits.max_open_candidates_total as usize
            || self.control_occupancy() + 2 > self.limits.max_control_records as usize
        {
            return Err(ContentCandidateError::Budget);
        }
        let mut ledger = FrameCredits::new(self.limits.grant, self.limits.permit_timeout_ms)?;
        ledger.observe_generation(self.last_candidate_generation);
        ledger.open(key, credits, now, || Ok(()))?;
        // One queued open reply and one held close obligation, regardless of credits.
        self.response_credits += 1;
        self.events.push_stream(ContentFrameStreamEvent {
            transaction,
            record: ContentFrameStreamRecord::Opened {
                key,
                credits,
                ttl_ms: self.limits.permit_timeout_ms,
            },
        });
        self.streams.last_stream_id = key.stream_id;
        self.streams.entries.insert(
            key.output,
            Entry {
                ledger,
                notice_transaction: transaction,
            },
        );
        Ok(())
    }

    /// Explicit close forbids new admissions but leaves admitted work cancellable
    /// or non-cancellable exactly as the ordinary candidate lifecycle requires.
    pub fn close_frame_stream(
        &mut self,
        transaction: TransactionId,
        key: FrameStreamKey,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(key.grant)?;
        self.time(now)?;
        self.expire(now)?;
        if !transaction.is_valid() {
            return Err(ContentCandidateError::Stale);
        }
        let entry = self
            .streams
            .entries
            .get_mut(&key.output)
            .ok_or(ContentCandidateError::Stale)?;
        entry.ledger.close(key, ContentReason::Cancelled)?;
        entry.notice_transaction = transaction;
        self.queue_stream_closes();
        Ok(())
    }

    /// Close streams whose exact allocation/output facts are no longer current.
    /// Already accepted immutable images and their leases are never retargeted.
    pub fn reconcile_frame_streams(
        &mut self,
        contexts: &[ContentCandidateContext<'_>],
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(self.limits.grant)?;
        self.time(now)?;
        self.expire(now)?;
        for entry in self.streams.entries.values_mut() {
            let key = entry.ledger.key().expect("opened stream has a key");
            if entry.ledger.phase() == FrameStreamPhase::Open
                && !contexts
                    .iter()
                    .any(|context| Self::stream_context_matches(key, context))
            {
                entry.ledger.close(key, ContentReason::AllocationLost)?;
            }
        }
        self.queue_stream_closes();
        Ok(())
    }

    pub fn pending_frame_stream_event(&self) -> Option<&ContentFrameStreamEvent> {
        self.events.stream_front()
    }

    pub fn take_frame_stream_event(&mut self) -> Option<ContentFrameStreamEvent> {
        let event = self.events.pop_stream()?;
        self.prune_closed_streams();
        Some(event)
    }

    pub(super) fn require_demand_mode(
        &self,
        output: ContentOutputId,
    ) -> Result<(), ContentCandidateError> {
        if self.streams.poisoned {
            return Err(ContentCandidateError::ClockRegression);
        }
        if self.streams.entries.contains_key(&output) {
            return Err(ContentCandidateError::Stale);
        }
        Ok(())
    }

    pub(super) fn stream_owns(&self, output: ContentOutputId, generation: u64) -> bool {
        self.streams
            .entries
            .get(&output)
            .is_some_and(|entry| entry.ledger.owns(generation))
    }

    pub(super) fn observe_stream_generation(&mut self) {
        for entry in self.streams.entries.values_mut() {
            entry
                .ledger
                .observe_generation(self.last_candidate_generation);
        }
    }

    pub(super) fn begin_frame_stream(
        &mut self,
        transaction: TransactionId,
        begin: ContentCandidateBegin,
        invalid: Option<ContentCandidateError>,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        self.begin_frame_stream_with(transaction, begin, invalid, now, || {
            Ok(FrameAdmissionReservation::default())
        })
    }

    pub(super) fn begin_frame_stream_with(
        &mut self,
        transaction: TransactionId,
        begin: ContentCandidateBegin,
        invalid: Option<ContentCandidateError>,
        now: u64,
        reserve_copy: impl FnOnce() -> Result<FrameAdmissionReservation, ContentCandidateError>,
    ) -> Result<(), ContentCandidateError> {
        let key = self.streams.entries[&begin.output]
            .ledger
            .key()
            .expect("opened stream key");
        if !transaction.is_valid()
            || key.stream_id != begin.pacing_permit
            || key.grant != begin.grant
            || key.facts_generation != begin.facts_generation
            || begin.candidate_generation <= self.last_candidate_generation
            || self.streams.entries[&begin.output].ledger.phase() != FrameStreamPhase::Open
            || self.assemblies.contains_key(&begin.output)
        {
            return Err(ContentCandidateError::Stale);
        }
        // A valid Begin identity is consumed even if its admission refuses.
        self.last_candidate_generation = begin.candidate_generation;
        let replacing = self.pending.contains_key(&begin.output);
        let occupied = self.control_occupancy() - usize::from(replacing) * 2;
        let budget = (self.pending.len() >= self.limits.max_pending_candidates_total as usize
            && !replacing)
            || occupied + 2 + usize::from(replacing) > self.limits.max_control_records as usize;
        let deadline = now.checked_add(u64::from(self.limits.candidate_timeout_ms));
        let refusal = invalid.or_else(|| {
            if begin.interaction_generation == 0
                || begin.surface_count != 1
                || begin.surface_count > self.limits.max_candidate_surfaces
                || begin.placement_count > self.limits.max_candidate_placements
                || begin.target_count > self.limits.max_candidate_targets
                || deadline.is_none()
            {
                Some(ContentCandidateError::Malformed)
            } else if budget {
                Some(ContentCandidateError::Budget)
            } else {
                None
            }
        });
        let mut reservation = FrameAdmissionReservation::default();
        let result = self
            .streams
            .entries
            .get_mut(&begin.output)
            .expect("stream selected")
            .ledger
            .admit(key, begin.candidate_generation, now, || {
                refusal.map_or(Ok(()), Err)?;
                reservation = reserve_copy()?;
                Ok(())
            });
        self.observe_stream_generation();
        result?;
        // Transfer the old pair to Superseded and reserve the new pair.
        // The old credit stays held until its terminal reaches the output.
        if let Some(candidate) = self.pending.remove(&begin.output) {
            self.response_credits -= 2;
            self.outcome(
                candidate.transaction,
                candidate.begin.candidate_generation,
                begin.output,
                4,
                ContentReason::Superseded,
                0,
                0,
                0,
            );
        }
        self.response_credits += 2;
        self.assemblies.insert(
            begin.output,
            Assembly {
                native_launcher: None,
                persistent_catalog: None,
                copy: reservation
                    .copy_job
                    .map(|job| FrameCopyState { job, stopped: None }),
                commit: reservation.commit,
                transaction,
                begin,
                deadline: deadline.expect("admission checked deadline"),
                next_ordinal: 0,
                surfaces: Vec::new(),
                placements: Vec::new(),
                targets: Vec::new(),
            },
        );
        Ok(())
    }

    pub(super) fn validate_stream_assembly(
        &self,
        assembly: &Assembly,
        context: &ContentCandidateContext<'_>,
    ) -> Result<(), ContentCandidateError> {
        if let Some(entry) = self.streams.entries.get(&assembly.begin.output)
            && entry.ledger.owns(assembly.begin.candidate_generation)
        {
            let key = entry.ledger.key().expect("opened stream key");
            if !Self::stream_context_matches(key, context)
                || assembly.surfaces.len() != 1
                || assembly.surfaces[0].allocation != key.allocation
            {
                return Err(ContentCandidateError::Stale);
            }
        }
        Ok(())
    }

    pub(super) fn frame_candidate_key(
        &self,
        output: ContentOutputId,
        generation: u64,
    ) -> Option<FrameStreamKey> {
        let entry = self.streams.entries.get(&output)?;
        entry
            .ledger
            .owns(generation)
            .then(|| entry.ledger.key().expect("opened stream key"))
    }

    /// Transfer stream output, returning credit only with a terminal outcome.
    ///
    /// The publisher must either transfer the whole event or leave output
    /// untouched on error. Prepared carries identity without credit or resource
    /// settlement. A successful terminal transfer also settles the new image
    /// and commits predecessor retirement. Queued outcomes are not replenishment, and an
    /// ordinary `take_event` cannot consume them. `now` is the owner's current
    /// monotonic time, not the time of the earlier native completion.
    pub fn transfer_frame_event_at(
        &mut self,
        resources: &mut ContentResourceStore,
        now: u64,
        publish: impl FnOnce(&ContentCandidateEvent) -> Result<(), ContentCandidateError>,
    ) -> Result<bool, ContentCandidateError> {
        let Some(mut event) = self.events.front().cloned() else {
            return Ok(false);
        };
        let Some(credit) = event.frame_credit else {
            return Err(ContentCandidateError::Stale);
        };
        let ShellContentRecord::CandidateOutcome(outcome) = &event.record else {
            return Err(ContentCandidateError::Malformed);
        };
        if resources.grant() != self.limits.grant {
            return Err(ContentCandidateError::Stale);
        }
        let generation = outcome.candidate_generation;
        // Clock poisoning closes admission but cannot strand existing terminals.
        let clock = self.time(now);
        if !self.streams.poisoned {
            clock?;
        }
        let entry = self
            .streams
            .entries
            .get_mut(&credit.key.output)
            .ok_or(ContentCandidateError::Stale)?;
        if outcome.kind == 1 {
            if entry.ledger.key() != Some(credit.key) || !entry.ledger.owns(generation) {
                return Err(ContentCandidateError::Stale);
            }
            publish(&event)?;
            self.events
                .pop_front()
                .expect("published Prepared remains owned");
            return Ok(true);
        }
        let mut publish = Some(publish);
        let mut result = entry
            .ledger
            .terminal(credit.key, generation, self.last_now, |deadline| {
                event.frame_credit = Some(ContentFrameCreditReturn {
                    key: credit.key,
                    expires_msec: deadline,
                });
                publish.take().expect("terminal published once")(&event)
            });
        if result == Err(ContentCandidateError::Malformed) && publish.is_some() {
            // Only a pre-publication deadline overflow can take this recovery.
            entry.ledger.close(credit.key, ContentReason::Malformed)?;
            result = entry
                .ledger
                .terminal(credit.key, generation, self.last_now, |_| {
                    event.frame_credit = Some(ContentFrameCreditReturn {
                        key: credit.key,
                        expires_msec: None,
                    });
                    publish.take().expect("overflow terminal not yet published")(&event)
                });
        }
        self.queue_stream_closes();
        result?;
        let ShellContentRecord::CandidateOutcome(outcome) = &event.record else {
            unreachable!("validated terminal");
        };
        resources.finish_frame_images(outcome);
        if let Some(owner) = event.retire {
            resources.commit_retire(&owner);
        }
        let transferred = self
            .events
            .pop_front()
            .expect("published FIFO head remains owned");
        if let Some(owner) = transferred.retire {
            self.presented_retire.retain(|(held, _)| *held != owner);
        }
        self.prune_closed_streams();
        Ok(true)
    }

    pub(super) fn expire_frame_streams(&mut self, now: u64) -> Result<(), ContentCandidateError> {
        let mut result = if self.streams.poisoned {
            Err(ContentCandidateError::ClockRegression)
        } else {
            Ok(())
        };
        for entry in self.streams.entries.values_mut() {
            if let Err(error) = entry.ledger.expire(now) {
                result = Err(error);
            }
        }
        self.queue_stream_closes();
        result
    }

    pub(super) fn poison_frame_streams(&mut self, _now: u64) {
        if self.streams.entries.is_empty() {
            return;
        }
        self.streams.poisoned = true;
        for entry in self.streams.entries.values_mut() {
            entry.ledger.poison();
        }
        self.queue_stream_closes();
    }

    fn queue_stream_closes(&mut self) {
        for entry in self.streams.entries.values_mut() {
            if entry.ledger.pending_close().is_some() {
                let transaction = entry.notice_transaction;
                entry
                    .ledger
                    .publish_close(|notice| {
                        self.events.push_stream(ContentFrameStreamEvent {
                            transaction,
                            record: ContentFrameStreamRecord::Closed(notice),
                        });
                        Ok(())
                    })
                    .expect("close control record was reserved at open");
                self.response_credits -= 1;
            }
        }
    }

    pub(super) fn prune_closed_streams(&mut self) {
        self.streams.entries.retain(|output, entry| {
            entry.ledger.phase() != FrameStreamPhase::Closed || self.events.has_output(*output)
        });
    }

    fn stream_context_matches(key: FrameStreamKey, context: &ContentCandidateContext<'_>) -> bool {
        key.output == context.output
            && key.facts_generation == context.facts_generation
            && context.allocations.iter().any(|allocation| {
                allocation.allocation == key.allocation && allocation.output == key.output
            })
    }
}
