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

use sophia_9p_client::pipeline::Pipeline;
use sophia_9p_records::{Fid, Tag};

use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::{
    ContentCandidate, ContentResourceId, ShellContentRecord, TransactionId,
};

use crate::wire::Outbound;
use crate::{ShellClientError, client_record};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubmissionPhase {
    Walk { tag: Tag, fid: Fid },
    Open { tag: Tag, fid: Fid },
    WriteRecord { tag: Tag, fid: Fid },
    WriteSubmit { tag: Tag, fid: Fid },
    AwaitSubmitted,
}

enum Current {
    Submission {
        bytes: Vec<u8>,
        submission_id: u64,
        phase: SubmissionPhase,
    },
    SlotWrite {
        resource: ContentResourceId,
        offset: u64,
        remaining: Vec<u8>,
        tag: Option<Tag>,
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
    Walk { tag: Tag, fid: Fid },
    Open { tag: Tag, fid: Fid },
    Read { tag: Tag, fid: Fid },
}

struct ObjectFetch {
    /// The `ObjectPublished` event's own sequence: acked once the fetch
    /// completes and its typed record is retained, never before.
    sequence: u64,
    phase: FetchPhase,
}

pub(crate) struct FileWire {
    pipeline: Pipeline,
    epoch: u64,
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
    ack_ready: u64,
    acked: u64,
    ack_tag: Option<Tag>,
    object_fetch: Option<ObjectFetch>,

    // Outbound: a single lane, mirroring `ClientOutbox`'s FIFO order.
    pending: VecDeque<QueuedKind>,
    staged: Option<QueuedKind>,
    staged_begin: Option<(u16, ContentResourceId)>,
    current: Option<Current>,

    uploads: [Option<Upload>; SHELL_FILE_MAX_UPLOAD_SLOTS as usize],

    /// Tags whose reply we do not otherwise act on (fire-and-forget clunks):
    /// still drained from the pipeline so its own bookkeeping never grows
    /// without bound.
    forgettable: HashSet<Tag>,

    peer_closed: bool,
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
        self.staged = None;
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
                self.staged = Some(QueuedKind::Record);
                Ok(vec![bytes])
            }
            Outbound::IndicatorActivation(..)
            | Outbound::ActionResponse { .. }
            | Outbound::CatalogCandidateGroup { .. }
            | Outbound::CatalogActionResponse { .. } => Err(ShellClientError::UnsupportedOnWire),
        }
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
                self.staged = Some(QueuedKind::SlotWrite {
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
                self.staged = Some(QueuedKind::Record);
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
                self.staged = Some(QueuedKind::Record);
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
                self.staged = Some(QueuedKind::Record);
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
                self.staged = Some(QueuedKind::Record);
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
                self.staged = Some(QueuedKind::Record);
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
                self.staged = Some(QueuedKind::Record);
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
        let Some(kind) = self.staged.take() else {
            return;
        };
        if let Some((slot, resource)) = self.staged_begin.take() {
            self.uploads[slot as usize] = Some(Upload {
                resource,
                state: UploadState::Pending,
            });
        }
        self.pending.push_back(kind);
    }
}
