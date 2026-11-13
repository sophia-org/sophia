//! G3-5d d-2b3d, test-support only: read-only journal facts, a witness of
//! the last Ready read reply this wrapper produced, and the retained
//! records decoded by identity. Nothing here drains, publishes, ACKs,
//! flushes, serves input or advances a clock. The retained table reads
//! custody bytes directly, below this wrapper, so it leaves the reply
//! witness and the pending count untouched.
use super::Journal;
use crate::owned_ring_protocol::Event;
use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::shell_files::{
    SHELL_FILE_MAX_CANDIDATE_BYTES, ShellFileClass, ShellFileCodecError, ShellFileKind,
    ShellFilePayloadError, ShellFileSubmitted, ShellFileTransactionRecord, ShellFrameRecord,
    decode_shell_file_allocation_result, decode_shell_file_record_at_revision,
    decode_shell_file_resource_status_at_revision, decode_shell_file_submitted,
    decode_shell_frame_record,
};
use sophia_protocol::{ShellContentRecord, TransactionId};
use std::time::Instant;

/// One export-produced Ready reply: the range it returned, not proof of a
/// socket write. The peer's later decode of the same range establishes
/// delivery. `serial` counts Ready evaluations of this journal from 1 and
/// is never reused; Pending evaluations are counted, not identified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadReplyWitness {
    pub serial: u64,
    pub epoch: u64,
    pub offset: u64,
    /// The count the reader asked for, before the visible clamp.
    pub count_requested: u32,
    pub count_clamped: u32,
    pub visible: u64,
    pub size: u64,
    pub returned: usize,
}

/// The journal's position, retention, visibility and reply state at one
/// read. `floor` is `tail - bytes`: retention is a contiguous prefix-ACK
/// run, so the retained bytes end at the tail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JournalFacts {
    pub epoch: u64,
    pub next_sequence: u64,
    pub tail: u64,
    pub floor: u64,
    pub records: usize,
    pub bytes: usize,
    pub held: usize,
    pub visible: u64,
    pub deferred: Option<(u64, u64)>,
    pub last_progress: Instant,
    pub ready: Option<ReadReplyWitness>,
    pub pending_evaluations: u64,
}

/// The typed identity a retained record carries, decoded whole.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordIdentity {
    Content {
        transaction: TransactionId,
        record: ShellContentRecord,
    },
    Frame(ShellFrameRecord),
    Submitted(ShellFileSubmitted),
    OwnedEvent(Event),
    /// A well-framed shell record of a kind no control identifies.
    Other,
}

/// One retained record: its journal sequence and epoch, its byte range
/// `[start, end)` and its identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetainedRecord {
    pub sequence: u64,
    pub epoch: u64,
    pub start: u64,
    pub end: u64,
    /// `None` for an owned-ring packet, which carries no shell header.
    pub kind: Option<ShellFileKind>,
    pub identity: RecordIdentity,
}

/// Retained bytes split at each record's declared size. A malformed or
/// unsupported record is reported as such, never as absent, and ends
/// the table.
#[derive(Clone, Debug, Eq, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a test-support table read between turns; boxing buys nothing"
)]
pub enum RetainedEntry {
    Record(RetainedRecord),
    Incomplete {
        start: u64,
        available: usize,
    },
    Undecodable {
        start: u64,
        available: usize,
        error: String,
    },
}

const CLASSES: [ShellFileClass; 3] = [
    ShellFileClass::Event,
    ShellFileClass::Object,
    ShellFileClass::Candidate,
];

impl Journal {
    pub(super) fn note_pending(&self) {
        self.pending_evaluations
            .set(self.pending_evaluations.get() + 1);
    }

    pub(super) fn note_outcome(
        &self,
        offset: u64,
        count_requested: u32,
        count_clamped: u32,
        visible: u64,
        outcome: &Result<ReadOutcome, Errno>,
    ) {
        match outcome {
            Ok(ReadOutcome::Ready(bytes)) => {
                let serial = self.ready_serial.get() + 1;
                self.ready_serial.set(serial);
                self.witness.set(Some(ReadReplyWitness {
                    serial,
                    epoch: self.custody.epoch(),
                    offset,
                    count_requested,
                    count_clamped,
                    visible,
                    size: self.custody.size(),
                    returned: bytes.len(),
                }));
            }
            Ok(ReadOutcome::Pending) => self.note_pending(),
            Err(_) => {}
        }
    }

    pub fn facts(&self) -> JournalFacts {
        let position = self.custody.position();
        JournalFacts {
            epoch: self.custody.epoch(),
            next_sequence: position.next_sequence,
            tail: position.tail,
            floor: position
                .tail
                .checked_sub(position.bytes as u64)
                .expect("retained bytes end at the tail"),
            records: position.records,
            bytes: position.bytes,
            held: self.held.iter().flatten().count(),
            visible: self.visible(),
            deferred: self.deferred,
            last_progress: self.last_progress,
            ready: self.witness.get(),
            pending_evaluations: self.pending_evaluations.get(),
        }
    }

    /// Every retained record, visible or not, from the floor to the tail.
    pub fn retained_table(&self) -> Vec<RetainedEntry> {
        let facts = self.facts();
        if facts.bytes == 0 {
            return Vec::new();
        }
        let count = u32::try_from(facts.bytes).expect("retained bytes fit a read");
        let mut scratch = Vec::new();
        let bytes = match self.custody.read_buffered(facts.floor, count, &mut scratch) {
            Ok(ReadOutcome::Ready(bytes)) => bytes,
            other => {
                return vec![RetainedEntry::Undecodable {
                    start: facts.floor,
                    available: 0,
                    error: format!("custody read: {other:?}"),
                }];
            }
        };
        let mut entries = Vec::new();
        let mut cursor = 0usize;
        let mut counted = facts.next_sequence - facts.records as u64;
        while cursor < bytes.len() {
            let start = facts.floor + cursor as u64;
            let rest = &bytes[cursor..];
            let available = rest.len();
            let Some(size) = rest.get(..4) else {
                entries.push(RetainedEntry::Incomplete { start, available });
                break;
            };
            let size = u32::from_le_bytes(size.try_into().expect("four bytes")) as usize;
            if size < 8 {
                entries.push(RetainedEntry::Undecodable {
                    start,
                    available,
                    error: format!("declared size {size}"),
                });
                break;
            }
            if size > available {
                entries.push(RetainedEntry::Incomplete { start, available });
                break;
            }
            let entry = self.decode_retained(counted, start, &rest[..size]);
            let stop = !matches!(entry, RetainedEntry::Record(_));
            entries.push(entry);
            if stop {
                break;
            }
            counted += 1;
            cursor += size;
        }
        entries
    }

    fn decode_retained(&self, counted: u64, start: u64, record: &[u8]) -> RetainedEntry {
        let available = record.len();
        let end = start + available as u64;
        let undecodable = |error: String| RetainedEntry::Undecodable {
            start,
            available,
            error,
        };
        let finish = |sequence: u64, epoch: u64, kind: Option<ShellFileKind>, identity| {
            if sequence != counted {
                return undecodable(format!(
                    "record sequence {sequence} at counted sequence {counted}"
                ));
            }
            RetainedEntry::Record(RetainedRecord {
                sequence,
                epoch,
                start,
                end,
                kind,
                identity,
            })
        };
        if record.get(4..8) == Some(b"D4E1") {
            return match Event::decode(record) {
                Some(event) => finish(
                    event.sequence,
                    event.identity.grant.connection_epoch,
                    None,
                    RecordIdentity::OwnedEvent(event),
                ),
                None => undecodable("owned-ring event".into()),
            };
        }
        let header = match CLASSES.iter().find_map(|class| {
            match decode_shell_file_record_at_revision(record, *class, self.revision) {
                Err(ShellFileCodecError::Class) => None,
                other => Some(other),
            }
        }) {
            Some(Ok(decoded)) => decoded.header,
            Some(Err(error)) => return undecodable(format!("envelope: {error:?}")),
            None => return undecodable("no class accepts the kind".into()),
        };
        let content = |decoded: Result<ShellFileTransactionRecord, ShellFilePayloadError>| {
            decoded
                .map(|r| RecordIdentity::Content {
                    transaction: r.transaction,
                    record: r.record,
                })
                .map_err(|error| format!("{:?}: {error:?}", header.kind))
        };
        let identity = match header.kind {
            ShellFileKind::ResourceStatus => content(
                decode_shell_file_resource_status_at_revision(record, self.revision),
            ),
            ShellFileKind::AllocationResult => content(decode_shell_file_allocation_result(record)),
            ShellFileKind::Submitted => decode_shell_file_submitted(record)
                .map(RecordIdentity::Submitted)
                .map_err(|error| format!("Submitted: {error:?}")),
            ShellFileKind::FrameOutcome
            | ShellFileKind::FrameStreamClosed
            | ShellFileKind::FrameStreamOpened
            | ShellFileKind::FrameRefused => decode_shell_frame_record(
                record,
                ShellFileClass::Event,
                self.revision,
                SHELL_FILE_MAX_CANDIDATE_BYTES,
            )
            .map(RecordIdentity::Frame)
            .map_err(|error| format!("{:?}: {error:?}", header.kind)),
            _ => Ok(RecordIdentity::Other),
        };
        match identity {
            Ok(identity) => finish(
                header.sequence,
                header.connection_epoch,
                Some(header.kind),
                identity,
            ),
            Err(error) => undecodable(error),
        }
    }
}
