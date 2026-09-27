//! The native 9P2000.L file wire for `sophia_shell_fs_v1`: the second
//! [`crate::wire::Wire`] variant. It speaks the shell file contract directly
//! over [`sophia_9p_client::pipeline::Pipeline`] -- no IPC frame, `IpcMessageKind` or
//! frame codec ever appears here.
//!
//! Everything client-to-server goes through the attach's single
//! `transaction`/`submit` custody cycle, one candidate record at a time (the
//! state machine mirrors `tests/support/shell_file_peer.rs`'s working sequence:
//! walk+open `transaction`, write the record, write `submit`, observe
//! `Submitted`, ack, clunk). Resource bytes travel as plain writes to a bound
//! `upload/N` slot, never as records. Exactly one `Tread` stays outstanding on
//! `events`; every decoded event becomes the same typed [`Inbound`] the socket
//! wire produces.
//!
//! This module owns the shared types and the connect-independent parts
//! (`encode`/`commit_encoded`); `connect`, the `poll_io` driving loop, the
//! submission state machine, upload-slot handling and event dispatch each
//! live in their own submodule, all as further `impl FileWire` blocks over
//! the one type defined here.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use sophia_9p_client::pipeline::Pipeline;
use sophia_9p_records::{Fid, Tag};

use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::{
    CatalogContentCandidate, ContentActionAck, ContentCandidate, ContentResourceId,
    ShellCatalogActionRecord, ShellContentRecord, ShellIndicatorActivation, TransactionId,
};

use crate::custody::Custody;
use crate::wire::Outbound;
use crate::{ShellClientError, client_record};

mod budget;
mod connect;
mod drive;
mod events;
mod submission;
mod uploads;

const O_RDONLY: u32 = 0;
const O_WRONLY: u32 = 1;
const O_RDWR: u32 = 2;
/// 9P2000.L Twrite overhead: the 7-byte message header plus fid(4) +
/// offset(8) + count(4).
const WRITE_OVERHEAD: u32 = 23;
/// Bounds how many internal state-machine rounds one `poll_io` call may
/// spend chaining local progress (reply arrived -> next request queued ->
/// maybe already answered). Never blocks; only limits work per call.
const MAX_ROUNDS: usize = 32;

fn is_disconnect(kind: std::io::ErrorKind) -> bool {
    use std::io::ErrorKind::*;
    matches!(
        kind,
        UnexpectedEof | WriteZero | BrokenPipe | ConnectionReset | ConnectionAborted
    )
}

/// One queued unit awaiting processing, mirroring `ClientOutbox`'s FIFO
/// one-for-one: the raw bytes live in the outbox, this only carries what the
/// file wire needs to drive it once its turn comes.
#[derive(Clone, Copy, Debug)]
enum QueuedKind {
    /// A whole encoded candidate record, ready for transaction+submit.
    Record,
    /// Exactly the payload to append to `resource`'s bound writer at
    /// `offset`, once that slot is open for writing.
    SlotWrite {
        resource: ContentResourceId,
        offset: u64,
    },
}

/// The first and largest pause before retrying a write the session refused
/// with `EAGAIN`. A retry also goes early once an event has been consumed,
/// since that is what frees journal room or a permit; it never goes in the
/// same pass that saw the refusal.
const RETRY_FIRST: Duration = Duration::from_millis(2);
const RETRY_MAX: Duration = Duration::from_millis(64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubmissionPhase {
    Walk {
        tag: Tag,
        fid: Fid,
    },
    Open {
        tag: Tag,
        fid: Fid,
    },
    WriteRecord {
        tag: Tag,
        fid: Fid,
    },
    /// `submit` is on the wire: from here the session may hold custody.
    WriteSubmit {
        tag: Tag,
        fid: Fid,
    },
    /// `submit` was refused with `EAGAIN`: nothing transferred, the staged
    /// record is kept, and the same submit goes again once `not_before`
    /// passes or an event is consumed after `progress`.
    RetryWait {
        fid: Fid,
        not_before: Instant,
        progress: u64,
        /// The `poll_io` pass that saw the refusal; a retry needs a later one.
        pass: u64,
    },
    /// `submit` returned `Rwrite`; waiting for the `Submitted` event.
    AwaitSubmitted,
}

enum Current {
    Submission {
        bytes: Vec<u8>,
        submission_id: u64,
        /// The record's kind, which its `Submitted` event must name.
        kind: ShellFileKind,
        ticket: u64,
        /// The exact `submit` bytes, once built, for an `EAGAIN` retry.
        submit: Vec<u8>,
        phase: SubmissionPhase,
        /// The `Submitted` event for this submission was observed (it may
        /// precede the `submit` reply). Custody is then settled for good.
        submitted: bool,
    },
    SlotWrite {
        resource: ContentResourceId,
        offset: u64,
        remaining: Vec<u8>,
        tag: Option<Tag>,
        ticket: u64,
        /// Set after `EAGAIN`: no write before this instant, nor in the
        /// same `poll_io` pass.
        not_before: Option<(Instant, u64)>,
        /// Some bytes already returned `Rwrite`.
        wrote_any: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SlotOpen {
    Walking { tag: Tag, fid: Fid },
    Opening { tag: Tag, fid: Fid },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UploadState {
    /// `ResourceBegin` submitted and committed client-side; awaiting the
    /// admitted (`status == 1`) `ResourceStatus` event.
    Pending,
    /// Admitted; walking and opening `upload/N` for writing.
    Opening(SlotOpen),
    /// The writer fid is open and ready for writes.
    Open(Fid),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Upload {
    resource: ContentResourceId,
    state: UploadState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FetchPhase {
    NotStarted,
    Walk {
        tag: Tag,
        fid: Fid,
    },
    Open {
        tag: Tag,
        fid: Fid,
    },
    /// Reading at `buf.len()`; a positive short read is not the end.
    Read {
        tag: Tag,
        fid: Fid,
        count: u32,
    },
    /// Exactly the feed's cap is held: one 1-byte read must find the end.
    Probe {
        tag: Tag,
        fid: Fid,
    },
}

/// Fetching the snapshot object one `ObjectPublished` announced.
struct ObjectFetch {
    feed: ShellFileKind,
    generation: u64,
    qid: u64,
    phase: FetchPhase,
    buf: Vec<u8>,
    /// Fetches restarted after a node-specific `ESTALE`/`EAGAIN`.
    restarts: u8,
}

/// Snapshot feeds the file wire fetches, and each one's encoded cap
/// (docs/sophia-shell-files.md, snapshot objects).
const FEEDS: [(ShellFileKind, usize); 3] = [
    (ShellFileKind::Outputs, SHELL_FILE_OUTPUTS_MAX_BYTES),
    (ShellFileKind::Catalog, SHELL_FILE_MAX_OBJECT_BYTES),
    (ShellFileKind::Indicators, SHELL_FILE_INDICATORS_MAX_BYTES),
];

fn feed_index(kind: ShellFileKind) -> Option<usize> {
    FEEDS.iter().position(|(feed, _)| *feed == kind)
}

/// How many times one announcement's fetch restarts after the node itself
/// answers `ESTALE` or `EAGAIN` before the connection fails closed.
const MAX_FETCH_RESTARTS: u8 = 2;

pub(crate) struct FileWire {
    pipeline: Pipeline,
    epoch: u64,
    /// The negotiated capabilities: which snapshot feeds may be announced.
    capabilities: u64,
    root: Fid,
    events_fid: Fid,
    submit_fid: Fid,
    ack_fid: Fid,
    upload_slots: u8,
    next_submission_id: u64,

    // Inbound: one Tread outstanding on `events`, whole-record reassembly.
    read_tag: Option<Tag>,
    read_offset: u64,
    event_buf: Vec<u8>,
    /// Every event through this sequence has been handled.
    ack_ready: u64,
    acked: u64,
    ack_tag: Option<Tag>,
    object_fetch: Option<ObjectFetch>,
    /// Per feed (`FEEDS` order): the ack bound an announcement not yet
    /// fetched imposes (its sequence minus one), kept at the earliest such
    /// announcement until the newest one is fetched and verified.
    holds: [Option<u64>; 3],
    /// Events handled so far; an `EAGAIN` retry waits for this to move.
    progress: u64,
    /// `poll_io` calls so far.
    pass: u64,
    /// A `Submitted` found among the buffered events while the submit reply
    /// was being settled, before normal intake reaches it; intake accepts
    /// that one event once.
    early_submitted: Option<(u64, ShellFileKind)>,

    // Outbound: a single lane, mirroring `ClientOutbox`'s FIFO order.
    pending: VecDeque<QueuedKind>,
    staged: Vec<QueuedKind>,
    staged_begin: Option<(u16, ContentResourceId)>,
    current: Option<Current>,
    /// The next `EAGAIN` backoff.
    backoff: Duration,
    /// Custody changes for `poll_io` to record in the connection's ledger.
    custody: Vec<(u64, Custody)>,
    /// Settled front units for `poll_io` to release from the outbox.
    retire: usize,

    uploads: [Option<Upload>; SHELL_FILE_MAX_UPLOAD_SLOTS as usize],

    /// Tags whose reply we do not otherwise act on (fire-and-forget clunks):
    /// still drained from the pipeline so its own bookkeeping never grows
    /// without bound.
    forgettable: HashSet<Tag>,

    peer_closed: bool,
    /// An event failed to parse or apply: bytes after it prove nothing.
    event_fault: bool,
    /// A connection-ending failure, returned again by every later call.
    fatal: Option<ShellClientError>,
}

/// The bound writer fid for `resource`, if its slot is open. A free function
/// over the field directly, so callers holding a live mutable borrow into a
/// *different* field of `FileWire` (such as `current`) can still call it
/// without conflicting with a `&self` method's whole-struct borrow.
fn open_fid_for(uploads: &[Option<Upload>], resource: ContentResourceId) -> Option<Fid> {
    uploads.iter().flatten().find_map(|upload| {
        (upload.resource == resource)
            .then_some(upload.state)
            .and_then(|state| match state {
                UploadState::Open(fid) => Some(fid),
                _ => None,
            })
    })
}

/// Merges a `ContentGroup`'s Begin/Chunk*/End records into one whole
/// candidate, refusing locally if the parts are incoherent. Unlike
/// `crate::candidate::metadata`, this keeps placements: the wire needs the
/// whole value, not just lifecycle-tracking metadata.
fn assemble_candidate(
    records: &[ShellContentRecord],
) -> Result<ContentCandidate, ShellClientError> {
    let incoherent = || ShellClientError::WrongDirection;
    if records.len() < 2 {
        return Err(incoherent());
    }
    let Some(ShellContentRecord::CandidateBegin(begin)) = records.first() else {
        return Err(incoherent());
    };
    let Some(ShellContentRecord::CandidateEnd(end)) = records.last() else {
        return Err(incoherent());
    };
    if begin.grant != end.grant
        || begin.candidate_generation != end.candidate_generation
        || begin.surface_count != end.surface_count
        || begin.placement_count != end.placement_count
        || begin.target_count != end.target_count
    {
        return Err(incoherent());
    }
    let mut surfaces = Vec::new();
    let mut placements = Vec::new();
    let mut targets = Vec::new();
    for (ordinal, record) in records[1..records.len() - 1].iter().enumerate() {
        let ShellContentRecord::CandidateChunk(chunk) = record else {
            return Err(incoherent());
        };
        if chunk.grant != begin.grant
            || chunk.candidate_generation != begin.candidate_generation
            || chunk.chunk_ordinal as usize != ordinal
        {
            return Err(incoherent());
        }
        surfaces.extend_from_slice(&chunk.surfaces);
        placements.extend_from_slice(&chunk.placements);
        targets.extend_from_slice(&chunk.targets);
        if surfaces.len() > begin.surface_count as usize
            || placements.len() > begin.placement_count as usize
            || targets.len() > begin.target_count as usize
        {
            return Err(incoherent());
        }
    }
    if surfaces.len() != begin.surface_count as usize
        || placements.len() != begin.placement_count as usize
        || targets.len() != begin.target_count as usize
    {
        return Err(incoherent());
    }
    Ok(ContentCandidate {
        grant: begin.grant,
        candidate_generation: begin.candidate_generation,
        output: begin.output,
        facts_generation: begin.facts_generation,
        pacing_permit: begin.pacing_permit,
        interaction_generation: begin.interaction_generation,
        surfaces,
        placements,
        targets,
    })
}

impl FileWire {
    /// Releases `fid` without waiting for the answer. On a pipeline that has
    /// already failed there is nothing left to release, and the failure that
    /// ended it is reported where it happened, not as this housekeeping step.
    /// On a live pipeline a clunk that cannot be queued (a local bound) is an
    /// error: the fid would otherwise stay open on the server, holding its
    /// pin or staging, while this side forgot it.
    fn forget(&mut self, fid: Fid) -> Result<(), ShellClientError> {
        if self.pipeline.is_poisoned() {
            return Ok(());
        }
        let tag = self.pipeline.clunk(fid)?;
        self.forgettable.insert(tag);
        Ok(())
    }

    pub(crate) fn peer_closed(&self) -> bool {
        self.peer_closed
    }

    fn next_header(&mut self, kind: ShellFileKind) -> ShellFileHeader {
        let submission_id = self.next_submission_id;
        self.next_submission_id += 1;
        ShellFileHeader {
            kind,
            connection_epoch: self.epoch,
            submission_id,
            sequence: 0,
        }
    }

    fn free_slot(&self) -> Option<u16> {
        (0..self.upload_slots)
            .map(u16::from)
            .find(|&slot| self.uploads[slot as usize].is_none())
    }

    pub(crate) fn encode(&mut self, outbound: Outbound) -> Result<Vec<Vec<u8>>, ShellClientError> {
        if let Some(error) = &self.fatal {
            return Err(error.clone());
        }
        self.staged.clear();
        self.staged_begin = None;
        match outbound {
            Outbound::Content(transaction, record) => self.encode_content(transaction, record),
            Outbound::ContentGroup(transaction, records) => {
                let candidate = assemble_candidate(&records)?;
                let header = self.next_header(ShellFileKind::Candidate);
                let bytes = encode_shell_file_candidate(
                    header,
                    &ShellFileCandidate {
                        transaction,
                        candidate,
                    },
                )?;
                Ok(self.stage_records(vec![bytes]))
            }
            Outbound::IndicatorActivation(transaction, activation) => {
                let bytes = self.indicator_activate(transaction, activation)?;
                Ok(self.stage_records(vec![bytes]))
            }
            Outbound::ActionResponse {
                transaction,
                ack,
                activation,
            } => {
                let mut records = vec![self.action_ack(transaction, ack)?];
                if let Some((transaction, activation)) = activation {
                    records.push(self.indicator_activate(transaction, activation)?);
                }
                Ok(self.stage_records(records))
            }
            Outbound::CatalogCandidateGroup {
                transaction,
                begin,
                chunks,
                end,
            } => {
                let mut records = vec![ShellContentRecord::CandidateBegin(begin.content)];
                records.extend(chunks.into_iter().map(ShellContentRecord::CandidateChunk));
                records.push(ShellContentRecord::CandidateEnd(end));
                let candidate = CatalogContentCandidate {
                    candidate: assemble_candidate(&records)?,
                    catalog_generation: begin.catalog_generation,
                };
                let header = self.next_header(ShellFileKind::CatalogCandidate);
                let bytes = encode_shell_file_catalog_candidate(
                    header,
                    &ShellFileCatalogCandidate {
                        transaction,
                        candidate,
                    },
                )?;
                Ok(self.stage_records(vec![bytes]))
            }
            Outbound::CatalogActionResponse {
                transaction,
                ack,
                activation,
            } => {
                let mut records = vec![self.action_ack(transaction, ack)?];
                if let Some((transaction, activation)) = activation {
                    let header = self.next_header(ShellFileKind::CatalogActivate);
                    records.push(encode_shell_file_catalog_action(
                        header,
                        &ShellFileCatalogActionRecord {
                            transaction,
                            record: ShellCatalogActionRecord::Activate(activation),
                        },
                    )?);
                }
                Ok(self.stage_records(records))
            }
        }
    }

    /// Stages whole records, each its own unit and its own submission.
    fn stage_records(&mut self, records: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
        self.staged
            .extend(std::iter::repeat_n(QueuedKind::Record, records.len()));
        records
    }

    fn action_ack(
        &mut self,
        transaction: TransactionId,
        ack: ContentActionAck,
    ) -> Result<Vec<u8>, ShellClientError> {
        let header = self.next_header(ShellFileKind::ActionAck);
        Ok(encode_shell_file_transaction(
            header,
            &ShellFileTransactionRecord {
                transaction,
                record: ShellContentRecord::ActionAck(ack),
            },
        )?)
    }

    fn indicator_activate(
        &mut self,
        transaction: TransactionId,
        activation: ShellIndicatorActivation,
    ) -> Result<Vec<u8>, ShellClientError> {
        let header = self.next_header(ShellFileKind::IndicatorActivate);
        Ok(encode_shell_file_indicator_activate(
            header,
            &ShellFileIndicatorActivate {
                transaction,
                activation,
            },
        )?)
    }

    fn encode_content(
        &mut self,
        transaction: TransactionId,
        record: ShellContentRecord,
    ) -> Result<Vec<Vec<u8>>, ShellClientError> {
        if !client_record(&record) {
            return Err(ShellClientError::WrongDirection);
        }
        match record {
            ShellContentRecord::ResourceChunk(chunk) => {
                self.staged.push(QueuedKind::SlotWrite {
                    resource: chunk.resource,
                    offset: chunk.offset,
                });
                Ok(vec![chunk.bytes])
            }
            ShellContentRecord::ResourceBegin(begin) => {
                let slot = self.free_slot().ok_or(ShellClientError::QueueSaturated)?;
                let resource = begin.resource;
                let header = self.next_header(ShellFileKind::ResourceBegin);
                let bytes = encode_shell_file_resource_begin(
                    header,
                    &ShellFileResourceBegin {
                        transaction,
                        slot,
                        record: ShellContentRecord::ResourceBegin(begin),
                    },
                )?;
                self.staged.push(QueuedKind::Record);
                self.staged_begin = Some((slot, resource));
                Ok(vec![bytes])
            }
            ShellContentRecord::AllocationRequest(_) => {
                let header = self.next_header(ShellFileKind::AllocationRequest);
                let bytes = encode_shell_file_allocation_request(
                    header,
                    ShellFileTransactionRecord {
                        transaction,
                        record,
                    },
                )?;
                self.staged.push(QueuedKind::Record);
                Ok(vec![bytes])
            }
            ShellContentRecord::ResourceEnd(_) => {
                let header = self.next_header(ShellFileKind::ResourceEnd);
                let bytes = encode_shell_file_resource_end(
                    header,
                    &ShellFileTransactionRecord {
                        transaction,
                        record,
                    },
                )?;
                self.staged.push(QueuedKind::Record);
                Ok(vec![bytes])
            }
            ShellContentRecord::ResourceCancel(_) => {
                let header = self.next_header(ShellFileKind::ResourceCancel);
                let bytes = encode_shell_file_resource_cancel(
                    header,
                    &ShellFileTransactionRecord {
                        transaction,
                        record,
                    },
                )?;
                self.staged.push(QueuedKind::Record);
                Ok(vec![bytes])
            }
            ShellContentRecord::ResourceRetire(_) => {
                let header = self.next_header(ShellFileKind::ResourceRetire);
                let bytes = encode_shell_file_resource_retire(
                    header,
                    &ShellFileTransactionRecord {
                        transaction,
                        record,
                    },
                )?;
                self.staged.push(QueuedKind::Record);
                Ok(vec![bytes])
            }
            ShellContentRecord::FrameDemand(_)
            | ShellContentRecord::FrameDemandCancel(_)
            | ShellContentRecord::ActionAck(_) => {
                let kind = match record {
                    ShellContentRecord::FrameDemand(_) => ShellFileKind::FrameDemand,
                    ShellContentRecord::FrameDemandCancel(_) => ShellFileKind::FrameDemandCancel,
                    ShellContentRecord::ActionAck(_) => ShellFileKind::ActionAck,
                    _ => unreachable!("matched above"),
                };
                let header = self.next_header(kind);
                let bytes = encode_shell_file_transaction(
                    header,
                    &ShellFileTransactionRecord {
                        transaction,
                        record,
                    },
                )?;
                self.staged.push(QueuedKind::Record);
                Ok(vec![bytes])
            }
            // CandidateBegin/Chunk/End alone (outside a ContentGroup) have no
            // single-record file-wire shape: only a whole candidate does.
            ShellContentRecord::CandidateBegin(_)
            | ShellContentRecord::CandidateChunk(_)
            | ShellContentRecord::CandidateEnd(_) => Err(ShellClientError::UnsupportedOnWire),
            _ => Err(ShellClientError::WrongDirection),
        }
    }

    pub(crate) fn commit_encoded(&mut self) {
        if let Some((slot, resource)) = self.staged_begin.take() {
            self.uploads[slot as usize] = Some(Upload {
                resource,
                state: UploadState::Pending,
            });
        }
        self.pending.extend(self.staged.drain(..));
    }
}
