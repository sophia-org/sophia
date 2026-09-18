//! Driving coordinator steps, and routing exactly what each one committed.
//!
//! ONE OWNER HOLDS EVERY PHASE. Intake that has been taken from the channel but
//! not committed, a batch that has been committed and whose decisions are still
//! being turned into commands, and commands minted and awaiting the order all
//! live in one structure behind one lock. Custody is therefore a single fact
//! rather than three that can disagree, and a stop can count what is still owed
//! without having to guess which phase lost it.
//!
//! NOTHING IS COMMITTED TWICE AND NOTHING IS TAKEN BEFORE ITS OUTCOME. A batch
//! leaves intake only once the assembly lock is already held, so a lock that
//! could not be taken leaves the batch where it was. A committed batch becomes
//! a staged batch carrying its own decisions and a cursor, so running out of
//! room or out of transaction identities halfway through leaves the remainder
//! staged rather than recommitted: `commit_authority_batches` has already run
//! for it and running it again would commit the same work a second time.
//!
//! EVERY FALLIBLE ACQUISITION HAPPENS BEFORE THE IRREVERSIBLE STEP. The
//! producer is acquired before any Session state lock, the boundary is read
//! once into a snapshot before the bridge is locked, and the assembly lock is
//! taken before the batch is popped. After a command has been handed to the
//! order there is no fallible acquisition left that could fail and make an
//! accepted command look like one worth retrying.

use sophia_protocol::{Rect, SurfaceId, TransactionId, TransactionOutcome};
use sophia_x_authority::{
    AdmissionRefusal, PrivateAdmittedConnection, XAuthorityClientControlCommand,
    XAuthorityControlKind, XAuthorityObservedTransactionBatch,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use super::control::{PrivateInputCommitted, PrivateInputControlError, PrivateInputSubmitted};
use super::handle::{PrivateInputHandle, PrivateInputUnavailable};
use super::submission::PrivateInputConnection;

/// The most the bridge will hold in minted commands before it stops committing.
///
/// A BOUND ON EFFECTS, NOT ON BATCHES. One batch can decide many effects, so
/// asking only whether there was any room at all and then committing a whole
/// drain of batches would let a single call overrun this by as much as those
/// batches happened to decide. Room is counted per effect as each is minted.
pub(super) const PRIVATE_INPUT_BRIDGE_BOUND: usize = 256;

/// One committed decision, before it has been given a command.
#[derive(Clone, Copy, Debug)]
struct PrivateInputDecision {
    committed_transaction: TransactionId,
    surface: SurfaceId,
    geometry: Option<Rect>,
    withdrawal: bool,
}

/// A batch the assembly has already committed, part-way through staging.
///
/// THE BATCH TRAVELS WITH ITS DECISIONS because classification still needs the
/// routes it carries. It is kept here rather than in intake precisely so that
/// nothing mistakes it for work still to be committed.
struct PrivateInputStagedBatch {
    batch: XAuthorityObservedTransactionBatch,
    decisions: Vec<PrivateInputDecision>,
    cursor: usize,
}

/// One committed decision, with the command it will be delivered as.
///
/// THE COMMAND IS MINTED ONCE. A retry that minted a new transaction would
/// leave the first one outstanding and unanswerable, and two acknowledgements
/// could then arrive for one committed decision.
#[derive(Clone, Copy, Debug)]
pub(super) struct PrivateInputBridgeEntry {
    committed_transaction: TransactionId,
    surface: SurfaceId,
    kind: XAuthorityControlKind,
    geometry: Option<Rect>,
    connection: PrivateInputConnection,
    transaction: TransactionId,
    command: XAuthorityClientControlCommand,
}

/// Every phase of work this service has taken and not yet handed on.
#[derive(Default)]
pub(super) struct PrivateInputBridge {
    /// Taken from the channel, not yet committed.
    intake: VecDeque<XAuthorityObservedTransactionBatch>,
    /// Committed, partly staged. Never recommitted.
    staged: Option<PrivateInputStagedBatch>,
    /// Minted, awaiting the order.
    entries: VecDeque<PrivateInputBridgeEntry>,
    /// Which surfaces are currently mapped, carried across batches.
    ///
    /// DURABLE, BECAUSE THE FACT IS. A drawing transaction arrives with no
    /// presentation observations at all: the runtime's apply path pushes the
    /// transaction without republishing surface state. Rebuilding the mapped
    /// set from each batch alone therefore found it empty on exactly the batch
    /// that applies the surface, and every applied surface was skipped, so a
    /// create/map/draw sequence never produced an admission.
    ///
    /// A map that arrived in an earlier FIFO batch is still true when a later
    /// batch commits, and this carries it. It does not let a later map
    /// authorise an earlier effect: batches are committed strictly in order and
    /// this is updated from a batch before that same batch's commits are
    /// routed, so a map arriving after an effect cannot reach back to it.
    mapped: BTreeSet<SurfaceId>,
}

impl PrivateInputBridge {
    /// Everything this still owes, across every phase.
    ///
    /// A stop reports this whole number: a batch taken from the channel and
    /// never committed is owed just as surely as a command minted and never
    /// delivered, and the X store has seen neither.
    pub(super) fn outstanding(&self) -> usize {
        self.intake.len()
            + self
                .staged
                .as_ref()
                .map(|staged| staged.decisions.len().saturating_sub(staged.cursor))
                .unwrap_or(0)
            + self.entries.len()
    }
}

/// Whether refusing this now says anything about refusing it later.
///
/// TERMINAL WORK IS NOT RETAINED. An entry whose client has gone, whose owner
/// is foreign, or whose identities are exhausted can never be delivered, and
/// holding it at the head of the queue would stop everything behind it for
/// good. It is reported as refused and released. Only a refusal that says
/// "later" keeps its place.
fn retryable(refusal: &AdmissionRefusal) -> bool {
    match refusal {
        AdmissionRefusal::Saturated
        | AdmissionRefusal::Unavailable
        | AdmissionRefusal::AuthorityUnreadable => true,
        AdmissionRefusal::Exhausted
        | AdmissionRefusal::ConsumerGone
        | AdmissionRefusal::ForeignServiceOwner => false,
    }
}

impl PrivateInputHandle {
    /// Commit what the frontend observed, and deliver what those commits
    /// decided.
    ///
    /// EXCLUSIVE TO THE CONTROLLER. This takes `&mut self` because it is not
    /// safe to run twice at once: an earlier version read the front of the
    /// queue, released the lock to submit, and took the lock again to pop, so
    /// two callers could read the same entry and submit the same command
    /// twice. Exclusivity is the fix that does not depend on remembering to
    /// hold a lock across a submit.
    pub fn apply_committed(
        &mut self,
        within: Duration,
    ) -> Result<PrivateInputCommitted, PrivateInputUnavailable> {
        let mut report = PrivateInputCommitted::default();

        // THE PRODUCER FIRST, BEFORE ANY SESSION STATE LOCK. Acquiring it after
        // the bridge was locked would put a fallible acquisition between a
        // decision and its delivery, and a failure there would look like work
        // worth retrying when the order had never been asked.
        let lease = self.runtime.owner.lease();
        let producer = match self.runtime.access.control_producer(&lease) {
            Ok(producer) => Some(producer),
            Err(_) => {
                report.refused.push(PrivateInputControlError::Ended);
                None
            }
        };
        // ONE READING OF THE BOUNDARY FOR THE WHOLE CALL, taken before the
        // bridge is held so the boundary's lock is never waited on underneath
        // this one.
        let live = self
            .runtime
            .participant
            .admitted()
            .map_err(|_| PrivateInputUnavailable)?;

        let mut bridge = self
            .runtime
            .bridge
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;

        if let Some(producer) = producer.as_ref() {
            pump_bridge(&mut bridge, producer, &lease, &live, &mut report);
        }

        // NEW WORK ONLY INTO THE ROOM THAT ACTUALLY REMAINS. Asking whether
        // intake was under its bound and then taking a whole drain bound's
        // worth let a queue bounded at 256 reach 511. Whatever is left unread
        // stays in its own channel, which is a queue already and does not need
        // a second copy of itself here.
        let room = PRIVATE_INPUT_BRIDGE_BOUND.saturating_sub(bridge.intake.len());
        if room > 0 {
            let batches = self.drain_transactions_limited(within, room);
            report.batches_observed = batches.len();
            bridge.intake.extend(batches);
        }

        self.advance_staging(&mut bridge, &live, &mut report)?;

        if let Some(producer) = producer.as_ref() {
            pump_bridge(&mut bridge, producer, &lease, &live, &mut report);
        }
        Ok(report)
    }

    /// How much work this service has taken and not yet handed on.
    pub fn outstanding(&self) -> Result<usize, PrivateInputUnavailable> {
        self.runtime
            .bridge
            .lock()
            .map(|held| held.outstanding())
            .map_err(|_| PrivateInputUnavailable)
    }

    /// Commit intake and turn committed decisions into commands, while there
    /// is room for them.
    fn advance_staging(
        &self,
        bridge: &mut PrivateInputBridge,
        live: &[PrivateAdmittedConnection],
        report: &mut PrivateInputCommitted,
    ) -> Result<(), PrivateInputUnavailable> {
        loop {
            if bridge.entries.len() >= PRIVATE_INPUT_BRIDGE_BOUND {
                return Ok(());
            }
            if bridge.staged.is_none() {
                if bridge.intake.is_empty() {
                    return Ok(());
                }
                // THE LOCK BEFORE THE POP. A batch is taken out of intake only
                // once the thing that will commit it is already held, so a lock
                // that could not be taken leaves the batch queued rather than
                // consumed by an attempt that never happened.
                let mut assembly = match self.runtime.assembly.lock() {
                    Ok(assembly) => assembly,
                    Err(_) => return Err(PrivateInputUnavailable),
                };
                let Some(batch) = bridge.intake.pop_front() else {
                    return Ok(());
                };
                let decisions = commit_batch(&mut assembly, &batch, &mut bridge.mapped, report);
                drop(assembly);
                bridge.staged = Some(PrivateInputStagedBatch {
                    batch,
                    decisions,
                    cursor: 0,
                });
            }
            if !self.stage_decisions(bridge, live, report)? {
                // Room or identities ran out; the remainder stays staged.
                return Ok(());
            }
        }
    }

    /// Mint commands for a staged batch's decisions, in order.
    ///
    /// Returns whether the staged batch was finished. Anything not minted stays
    /// behind the cursor, and the batch is never committed again.
    fn stage_decisions(
        &self,
        bridge: &mut PrivateInputBridge,
        live: &[PrivateAdmittedConnection],
        report: &mut PrivateInputCommitted,
    ) -> Result<bool, PrivateInputUnavailable> {
        let mut admitted = self
            .runtime
            .admitted_surfaces
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;
        let Some(staged) = bridge.staged.as_mut() else {
            return Ok(true);
        };
        while staged.cursor < staged.decisions.len() {
            if bridge.entries.len() >= PRIVATE_INPUT_BRIDGE_BOUND {
                return Ok(false);
            }
            let decision = staged.decisions[staged.cursor];
            // DECIDED WITHOUT MOVING THE LEDGER. An earlier version recorded
            // the admission or the withdrawal here, before it knew whether a
            // transaction identity or a command existed for it. A failed mint
            // then left the ledger describing a surface as admitted that had
            // never been admitted, or as withdrawn while its entry was still
            // staged -- a ledger and a staged decision describing different
            // phases of the same work. Nothing moves until the entry is real.
            let Some((kind, connection)) = decide(
                &staged.batch,
                &admitted,
                live,
                decision.surface,
                decision.withdrawal,
            ) else {
                staged.cursor += 1;
                continue;
            };
            let Some(transaction) = self.runtime.next_transaction() else {
                // TERMINAL, AND THE WORK STAYS STAGED, with the ledger
                // untouched so a later call decides it exactly as this one did.
                report.refused.push(PrivateInputControlError::Exhausted);
                return Ok(false);
            };
            let command = match command_for(
                kind,
                transaction,
                decision.surface,
                decision.geometry,
                connection,
            ) {
                Some(command) => command,
                None => {
                    staged.cursor += 1;
                    continue;
                }
            };
            // The entry exists, so now the ledger may follow it.
            apply_ledger(&mut admitted, decision.surface, kind, connection);
            bridge.entries.push_back(PrivateInputBridgeEntry {
                committed_transaction: decision.committed_transaction,
                surface: decision.surface,
                kind,
                geometry: decision.geometry,
                connection,
                transaction,
                command,
            });
            staged.cursor += 1;
        }
        bridge.staged = None;
        Ok(true)
    }
}

/// Commit one batch and read out what it decided.
///
/// The assembly is held throughout, so committed geometry is read from the
/// state this batch produced rather than from whatever a later batch left.
fn commit_batch(
    assembly: &mut sophia_engine::QueuedHeadlessCompositorBackendAssembly,
    batch: &XAuthorityObservedTransactionBatch,
    mapped: &mut BTreeSet<SurfaceId>,
    report: &mut PrivateInputCommitted,
) -> Vec<PrivateInputDecision> {
    // THE LEDGER IS UPDATED FROM THIS BATCH BEFORE THIS BATCH IS ROUTED, and
    // only from facts the batch actually carries. A batch with no presentation
    // observations says nothing about mapping and therefore changes nothing,
    // which is what lets a drawing transaction be routed against a map that
    // arrived earlier.
    //
    // An observation is the runtime's current state for that surface, so it
    // both sets and clears. A deferred policy map is the exception and is
    // applied after: while policy maps are deferred the surface is recorded as
    // pending rather than mapped, so `mapped` stays false and the batch carries
    // a Request instead. That Request is what authorises the admission, and
    // `mapped` becomes true only once the admission succeeds -- requiring the
    // observation for that branch would wait on a fact the admission itself
    // produces.
    for seen in &batch.surface_presentations {
        if seen.mapped {
            mapped.insert(seen.surface);
        } else {
            mapped.remove(&seen.surface);
        }
    }
    let mut withdrawn = BTreeSet::new();
    for intent in &batch.presentation_intents {
        match intent.kind {
            sophia_protocol::SurfacePresentationIntentKind::Request => {
                mapped.insert(intent.surface);
            }
            sophia_protocol::SurfacePresentationIntentKind::Withdraw => {
                withdrawn.insert(intent.surface);
            }
        }
    }
    withdrawn.extend(batch.removed_surfaces.iter().copied());
    // A withdrawn surface is no longer mapped. Its own incarnation is in its
    // `SurfaceId`, so a successor surface starts unmapped rather than
    // inheriting this one's fact.
    for surface in &withdrawn {
        mapped.remove(surface);
    }

    let intake = sophia_engine::AuthorityTransactionIntake::new(
        batch.transaction,
        batch.transactions.clone(),
    )
    .with_surface_removals(batch.removed_surfaces.clone());

    let commits = assembly.commit_authority_batches(std::slice::from_ref(&intake));
    report.commits += commits.len();

    let mut decisions = Vec::new();
    for commit in &commits {
        if commit.outcome != TransactionOutcome::Committed {
            continue;
        }
        report.committed += 1;
        for surface in &commit.applied_surfaces {
            if !mapped.contains(surface) {
                continue;
            }
            let Some(geometry) = assembly
                .committed_surfaces()
                .iter()
                .find(|held| held.surface == *surface)
                .map(|held| held.geometry)
            else {
                continue;
            };
            decisions.push(PrivateInputDecision {
                committed_transaction: commit.transaction,
                surface: *surface,
                geometry: Some(geometry),
                withdrawal: false,
            });
        }
        // REMOVALS TRAVEL SEPARATELY. A removal-only batch applies no
        // surface, so a withdrawal read out of `applied_surfaces` would
        // never be seen at all.
        for surface in &withdrawn {
            decisions.push(PrivateInputDecision {
                committed_transaction: commit.transaction,
                surface: *surface,
                geometry: None,
                withdrawal: true,
            });
        }
    }
    decisions
}

/// Decide which effect this surface calls for and which connection owns it.
///
/// READS THE LEDGER, NEVER MOVES IT. Moving it is [`apply_ledger`], which runs
/// only once the entry this decision produces actually exists.
fn decide(
    batch: &XAuthorityObservedTransactionBatch,
    admitted: &BTreeMap<SurfaceId, PrivateInputConnection>,
    live: &[PrivateAdmittedConnection],
    surface: SurfaceId,
    withdrawal: bool,
) -> Option<(XAuthorityControlKind, PrivateInputConnection)> {
    if withdrawal {
        // THE ROUTE THAT ADMITTED IT, not one read from the withdrawing batch,
        // which no longer carries it.
        return admitted
            .get(&surface)
            .map(|connection| (XAuthorityControlKind::WithdrawSurface, *connection));
    }
    let route = batch
        .surface_routes
        .iter()
        .find(|route| route.surface == surface)?;
    let admission = route.admission?;
    // THE BOUNDARY'S OWN ROW FOR THIS ADMISSION. The boundary currently
    // initialises a binding's generation from the admission's auth provenance,
    // so that number would agree; what makes this the right source is not a
    // disagreement between clocks but that the row is the record of an
    // admission the boundary actually holds. The exact admission is what
    // separates a reconnecting client from the one that went, and a row read
    // here cannot describe a connection the boundary has no binding for.
    let seen = live.iter().find(|seen| {
        seen.client == route.client && seen.admission == admission.client_id && !seen.closed
    })?;
    let connection = PrivateInputConnection {
        client: seen.client,
        admission: seen.admission,
        connection_generation: seen.connection_generation,
    };
    let kind = match admitted.get(&surface) {
        // A surface whose admission belongs to another connection is a new
        // admission, not a configure of somebody else's window.
        Some(held) if held.admission == connection.admission => {
            XAuthorityControlKind::ConfigureSurface
        }
        _ => XAuthorityControlKind::AdmitSurface,
    };
    Some((kind, connection))
}

/// Move the ledger to match an effect that has actually been staged.
///
/// One effect at a time, so two commits of the same surface in one call cannot
/// both be admissions.
fn apply_ledger(
    admitted: &mut BTreeMap<SurfaceId, PrivateInputConnection>,
    surface: SurfaceId,
    kind: XAuthorityControlKind,
    connection: PrivateInputConnection,
) {
    match kind {
        XAuthorityControlKind::WithdrawSurface => {
            admitted.remove(&surface);
        }
        _ => {
            admitted.insert(surface, connection);
        }
    }
}

/// The command one classified decision is delivered as.
fn command_for(
    kind: XAuthorityControlKind,
    transaction: TransactionId,
    surface: SurfaceId,
    geometry: Option<Rect>,
    connection: PrivateInputConnection,
) -> Option<XAuthorityClientControlCommand> {
    let command = match (kind, geometry) {
        (XAuthorityControlKind::AdmitSurface, Some(geometry)) => {
            sophia_x_authority::XAuthorityControlCommand::AdmitSurface {
                transaction,
                surface,
                geometry,
            }
        }
        (XAuthorityControlKind::ConfigureSurface, Some(geometry)) => {
            sophia_x_authority::XAuthorityControlCommand::ConfigureSurface {
                transaction,
                surface,
                geometry,
            }
        }
        (XAuthorityControlKind::WithdrawSurface, _) => {
            sophia_x_authority::XAuthorityControlCommand::WithdrawSurface {
                transaction,
                surface,
            }
        }
        _ => return None,
    };
    Some(XAuthorityClientControlCommand {
        client: connection.client,
        command,
    })
}

/// Deliver from the front of the bridge until the order says "later".
///
/// THE LOCK IS HELD ACROSS THE TRANSFER AND THE POP. The bridge is already
/// owned by the caller for the whole of this, and the producer was acquired
/// before it, so there is nothing fallible between handing a command to the
/// order and removing it from the queue. An accepted command can therefore
/// never be left behind to be sent a second time.
fn pump_bridge(
    bridge: &mut PrivateInputBridge,
    producer: &sophia_x_authority::PrivateControlProducer,
    lease: &sophia_x_authority::PrivateServiceLease<'_>,
    live: &[PrivateAdmittedConnection],
    report: &mut PrivateInputCommitted,
) {
    // COPIED UNDER THE HELD LOCK, and the lock is never released inside this
    // loop, so nothing can pop between reading the front and removing it.
    while let Some(entry) = bridge.entries.front().copied() {
        // THE ADMISSION IS RECHECKED AGAINST THE READING TAKEN FOR THIS CALL. A
        // connection that ended before this reading is refused rather than
        // served through a number a successor now holds.
        let present = live.iter().any(|seen| {
            seen.client == entry.connection.client
                && seen.admission == entry.connection.admission
                && !seen.closed
        });
        if !present {
            bridge.entries.pop_front();
            report.effects.push(super::PrivateInputCommittedEffect::new(
                entry.committed_transaction,
                entry.surface,
                entry.kind,
                entry.geometry,
                None,
            ));
            report
                .refused
                .push(PrivateInputControlError::ConnectionGone);
            continue;
        }
        match producer.submit(lease, entry.command) {
            Ok(_sequence) => {
                bridge.entries.pop_front();
                report.effects.push(super::PrivateInputCommittedEffect::new(
                    entry.committed_transaction,
                    entry.surface,
                    entry.kind,
                    entry.geometry,
                    Some(PrivateInputSubmitted {
                        transaction: entry.transaction,
                        surface: entry.surface,
                        kind: entry.kind,
                    }),
                ));
            }
            Err((refusal, command)) => {
                let keep = retryable(&refusal);
                report.effects.push(super::PrivateInputCommittedEffect::new(
                    entry.committed_transaction,
                    entry.surface,
                    entry.kind,
                    entry.geometry,
                    None,
                ));
                report
                    .refused
                    .push(PrivateInputControlError::Refused(refusal, command));
                if keep {
                    // STOPS HERE, AND EVERYTHING BEHIND IT STAYS BEHIND IT.
                    // Letting a later effect through would apply a configure to
                    // a window whose own admission the order had just deferred.
                    return;
                }
                // Terminal: it can never be delivered, so it is released rather
                // than left to block the queue for good.
                bridge.entries.pop_front();
            }
        }
    }
}
