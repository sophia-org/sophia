//! Bounded event custody for the output file export. Domain admission and
//! replay live in OutputConnectionState; fid, staging and read/ack lifecycle
//! belong to the export. Every outstanding proposal owns one terminal credit.
use std::collections::BTreeSet;

use sophia_9p::journal::{Journal, JournalBounds, JournalPosition, PreparedBatch};
use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::output_files::*;
use sophia_protocol::{
    OutputV1Outcome, OutputV1OutcomeKind, OutputV1ServerWelcome,
    SOPHIA_OUTPUT_OUTCOME_REASON_STALE, TransactionId,
};

const OUTCOME_BYTES: usize = OUTPUT_FILE_HEADER_BYTES + 24;
const MAX_PENDING: usize = 2; // One active and one replaceable queued proposal.

pub const OUTPUT_FILE_JOURNAL_BOUNDS: JournalBounds = JournalBounds {
    records: OUTPUT_FILE_MAX_JOURNAL_RECORDS as usize,
    bytes: OUTPUT_FILE_MAX_JOURNAL_BYTES as usize,
};

pub struct OutputFileJournal {
    journal: Journal,
    bounds: JournalBounds,
    pending: BTreeSet<TransactionId>,
}

/// The queued proposal replaced by an admission. The owner supplies the
/// current topology epoch; custody emits its Stale outcome in the same batch
/// as the new proposal's Submitted receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFileReplacement {
    pub transaction: TransactionId,
    pub topology_epoch: u64,
}

/// Reserve first, then perform domain admission, then commit. Dropping this
/// guard changes neither the journal nor its outstanding terminal credits.
pub struct PreparedOutputAdmission<'a> {
    batch: PreparedBatch<'a>,
    pending: &'a mut BTreeSet<TransactionId>,
    transaction: TransactionId,
    replaced: Option<TransactionId>,
    receipt: u64,
}

impl PreparedOutputAdmission<'_> {
    pub fn commit(self) -> u64 {
        self.batch.commit();
        if let Some(old) = self.replaced {
            self.pending.remove(&old);
        }
        self.pending.insert(self.transaction);
        self.receipt
    }
}

impl OutputFileJournal {
    pub fn new(epoch: u64, bounds: JournalBounds) -> Result<Self, Errno> {
        if epoch == 0
            || !(2..=OUTPUT_FILE_JOURNAL_BOUNDS.records).contains(&bounds.records)
            || !(104..=OUTPUT_FILE_JOURNAL_BOUNDS.bytes).contains(&bounds.bytes)
        {
            return Err(Errno::EINVAL);
        }
        Ok(Self {
            journal: Journal::new(epoch),
            bounds,
            pending: BTreeSet::new(),
        })
    }

    pub fn position(&self) -> JournalPosition {
        self.journal.position()
    }

    pub fn reserved_outcomes(&self) -> usize {
        self.pending.len()
    }

    pub fn read(&self, offset: u64, count: u32) -> Result<ReadOutcome, Errno> {
        self.journal.read(offset, count)
    }

    pub fn acknowledge(&mut self, ack: OutputFileAck) -> Result<bool, Errno> {
        // Releasing journal retention never releases an unsettled outcome.
        self.journal.ack(ack.connection_epoch, ack.sequence)
    }

    fn event(&self, kind: OutputFileKind, body: &[u8], index: u64) -> Result<Vec<u8>, Errno> {
        let sequence = self
            .journal
            .next_sequence()
            .checked_add(index)
            .ok_or(Errno::ENOSPC)?;
        encode_output_file_record(
            OutputFileHeader {
                kind,
                connection_epoch: self.journal.epoch(),
                submission_id: 0,
                sequence,
            },
            body,
        )
        .map_err(|_| Errno::EINVAL)
    }

    fn receipt(
        &self,
        submission_id: u64,
        candidate_kind: OutputFileKind,
    ) -> Result<Vec<u8>, Errno> {
        let body = encode_output_file_submitted(OutputFileSubmitted {
            submission_id,
            candidate_kind,
        })
        .map_err(|_| Errno::EINVAL)?;
        self.event(OutputFileKind::Submitted, &body, 0)
    }

    fn available(&self, records: &[Vec<u8>], pending: usize) -> Result<JournalBounds, Errno> {
        let reserve_bytes = pending * OUTCOME_BYTES;
        // Reserve sequence/offset space as well as retained bytes. A terminal
        // credit must remain usable even at the end of either integer space.
        let bytes: usize = records.iter().map(Vec::len).sum();
        let position = self.journal.position();
        position
            .next_sequence
            .checked_add((records.len() + pending) as u64)
            .ok_or(Errno::ENOSPC)?;
        position
            .tail
            .checked_add((bytes + reserve_bytes) as u64)
            .ok_or(Errno::ENOSPC)?;
        Ok(JournalBounds {
            records: self
                .bounds
                .records
                .checked_sub(pending)
                .ok_or(Errno::EAGAIN)?,
            bytes: self
                .bounds
                .bytes
                .checked_sub(reserve_bytes)
                .ok_or(Errno::EAGAIN)?,
        })
    }

    pub fn prepare_proposal(
        &mut self,
        submission_id: u64,
        transaction: TransactionId,
        replacement: Option<OutputFileReplacement>,
    ) -> Result<PreparedOutputAdmission<'_>, Errno> {
        if !transaction.is_valid() || self.pending.contains(&transaction) {
            return Err(Errno::EINVAL);
        }
        if replacement.is_some_and(|old| !self.pending.contains(&old.transaction)) {
            return Err(Errno::EINVAL);
        }
        let future_pending = self.pending.len() + 1 - usize::from(replacement.is_some());
        if future_pending > MAX_PENDING {
            return Err(Errno::EAGAIN);
        }
        let receipt = self.journal.next_sequence();
        let mut records = vec![self.receipt(submission_id, OutputFileKind::Proposal)?];
        if let Some(old) = replacement {
            let outcome = OutputV1Outcome {
                connection_epoch: self.journal.epoch(),
                topology_epoch: old.topology_epoch,
                kind: OutputV1OutcomeKind::Stale,
                reason: SOPHIA_OUTPUT_OUTCOME_REASON_STALE,
            };
            let body =
                encode_output_file_outcome(old.transaction, outcome).map_err(|_| Errno::EINVAL)?;
            records.push(self.event(OutputFileKind::Outcome, &body, 1)?);
        }
        let bounds = self.available(&records, future_pending)?;
        let batch = self.journal.prepare_batch(records, bounds)?;
        Ok(PreparedOutputAdmission {
            batch,
            pending: &mut self.pending,
            transaction,
            replaced: replacement.map(|old| old.transaction),
            receipt,
        })
    }

    /// Reserve an immediate semantic rejection without taking either pending
    /// proposal's terminal credit. The adapter commits this only after the
    /// domain owner has consumed the new identity and rejected its candidate.
    pub fn prepare_rejection(
        &mut self,
        submission_id: u64,
        transaction: TransactionId,
        outcome: OutputV1Outcome,
    ) -> Result<PreparedBatch<'_>, Errno> {
        if outcome.connection_epoch != self.journal.epoch() {
            return Err(Errno::ESTALE);
        }
        if outcome.kind != OutputV1OutcomeKind::Rejected || self.pending.contains(&transaction) {
            return Err(Errno::EINVAL);
        }
        let body = encode_output_file_outcome(transaction, outcome).map_err(|_| Errno::EINVAL)?;
        let records = vec![
            self.receipt(submission_id, OutputFileKind::Proposal)?,
            self.event(OutputFileKind::Outcome, &body, 1)?,
        ];
        let bounds = self.available(&records, self.pending.len())?;
        self.journal.prepare_batch(records, bounds)
    }

    /// Spend exactly the matching proposal's reserved terminal credit. Wrong
    /// epochs/identities and malformed outcomes leave the reservation intact.
    pub fn finish(
        &mut self,
        transaction: TransactionId,
        outcome: OutputV1Outcome,
    ) -> Result<u64, Errno> {
        if outcome.connection_epoch != self.journal.epoch() {
            return Err(Errno::ESTALE);
        }
        if !self.pending.contains(&transaction) {
            return Err(Errno::EINVAL);
        }
        let body = encode_output_file_outcome(transaction, outcome).map_err(|_| Errno::EINVAL)?;
        let records = vec![self.event(OutputFileKind::Outcome, &body, 0)?];
        let bounds = self.available(&records, self.pending.len() - 1)?;
        let sequence = self.journal.prepare_batch(records, bounds)?.commit();
        self.pending.remove(&transaction);
        Ok(sequence)
    }

    pub fn publish_topology(&mut self, publication: OutputFilePublication) -> Result<u64, Errno> {
        let body = encode_output_file_publication(publication).map_err(|_| Errno::EINVAL)?;
        let records = vec![self.event(OutputFileKind::ObjectPublished, &body, 0)?];
        let bounds = self.available(&records, self.pending.len())?;
        Ok(self.journal.prepare_batch(records, bounds)?.commit())
    }

    pub fn negotiated(
        &mut self,
        submission_id: u64,
        welcome: OutputV1ServerWelcome,
        publication: OutputFilePublication,
    ) -> Result<u64, Errno> {
        if welcome.connection_epoch != self.journal.epoch() {
            return Err(Errno::ESTALE);
        }
        let welcome = encode_output_file_negotiated(welcome).map_err(|_| Errno::EINVAL)?;
        let publication = encode_output_file_publication(publication).map_err(|_| Errno::EINVAL)?;
        let records = vec![
            self.receipt(submission_id, OutputFileKind::Negotiate)?,
            self.event(OutputFileKind::Negotiated, &welcome, 1)?,
            self.event(OutputFileKind::ObjectPublished, &publication, 2)?,
        ];
        let bounds = self.available(&records, self.pending.len())?;
        Ok(self.journal.prepare_batch(records, bounds)?.commit())
    }

    pub fn refused(&mut self, submission_id: u64, reason: OutputFileRefusal) -> Result<u64, Errno> {
        let records = vec![
            self.receipt(submission_id, OutputFileKind::Negotiate)?,
            self.event(
                OutputFileKind::Refused,
                &encode_output_file_refused(reason),
                1,
            )?,
        ];
        let bounds = self.available(&records, self.pending.len())?;
        Ok(self.journal.prepare_batch(records, bounds)?.commit())
    }
}
