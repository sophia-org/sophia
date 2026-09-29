//! Custody for one already-authorized output connection. Socket credential
//! admission remains with the transport; attach strings grant no authority.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use sophia_9p::connection::ConnectionId;
use sophia_9p::journal::{Staging, StagingBounds};
use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::output_files::*;
use sophia_protocol::{OutputAuthoritySnapshot, OutputV1Outcome, OutputV1Snapshot, TransactionId};

use crate::output_file_reads::OutputFileReads;
use crate::{AdmittedOutputProposal, OutputFileAdmission, OutputFileSubmission};

mod filesystem;
pub use filesystem::{OutputFileHandle, OutputFileNode};

/// Share this allocator across reconnects. Neither fid reuse nor a fresh
/// connection epoch can cause a published Qid to name different bytes.
#[derive(Clone)]
pub struct OutputFileQids(Arc<AtomicU64>);

impl Default for OutputFileQids {
    fn default() -> Self {
        Self(Arc::new(AtomicU64::new(1)))
    }
}

impl OutputFileQids {
    fn allocate(&self, count: u64) -> Result<u64, Errno> {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(count)
            })
            .map_err(|_| Errno::ENOSPC)
    }
}

struct Topology {
    qid: u64,
    bytes: Vec<u8>,
    snapshot: OutputAuthoritySnapshot,
}

struct Publication {
    sequence: u64,
    qid: u64,
    read: bool,
}

pub struct OutputFileExport {
    epoch: u64,
    qids: OutputFileQids,
    base: u64,
    limits: OutputFileLimits,
    limits_bytes: Vec<u8>,
    admission: OutputFileAdmission,
    connection: Option<ConnectionId>,
    attached: bool,
    revoked: bool,
    topology: Arc<Topology>,
    topology_open: Option<u64>,
    topology_read: bool,
    publication: Option<Publication>,
    staging: Option<Staging>,
    staged_submitted: bool,
    receipt: Option<u64>,
    reads: OutputFileReads,
    ack_deadline: Option<Instant>,
    refused: bool,
    delivery: Option<OutputFileSubmission>,
}

impl OutputFileExport {
    pub fn new(
        epoch: u64,
        limits: OutputFileLimits,
        snapshot: OutputAuthoritySnapshot,
        qids: OutputFileQids,
    ) -> Result<Self, Errno> {
        let admission = OutputFileAdmission::new(epoch, limits)?;
        let limits_bytes = object(
            epoch,
            OutputFileKind::Limits,
            &encode_output_file_limits(limits).map_err(|_| Errno::EINVAL)?,
        )?;
        let bytes = topology_bytes(epoch, &snapshot)?;
        let base = qids.allocate(9)?;
        Ok(Self {
            epoch,
            qids,
            base,
            limits,
            limits_bytes,
            admission,
            connection: None,
            attached: false,
            revoked: false,
            topology: Arc::new(Topology {
                qid: base + 8,
                bytes,
                snapshot,
            }),
            topology_open: None,
            topology_read: false,
            publication: None,
            staging: None,
            staged_submitted: false,
            receipt: None,
            reads: OutputFileReads::new(),
            ack_deadline: None,
            refused: false,
            delivery: None,
        })
    }

    /// Bind only the connection admitted by the protected endpoint. This
    /// operation cannot replace an existing grant or revive a revoked export.
    pub fn bind_connection(&mut self, connection: ConnectionId) -> Result<(), Errno> {
        if self.revoked || self.connection.is_some() {
            return Err(Errno::EACCES);
        }
        self.connection = Some(connection);
        Ok(())
    }

    pub fn revoke(&mut self) {
        self.revoked = true;
        self.staging = None;
        self.delivery = None;
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked
    }

    /// The worker must poll this even when the peer sends nothing.
    pub fn expire(&mut self, now: Instant) {
        if self.ack_deadline.is_some_and(|deadline| now >= deadline) {
            self.revoke();
        }
        if !self.staged_submitted && self.staging.as_ref().is_some_and(|s| s.expired(now)) {
            self.staging = None;
        }
    }

    pub fn wait(&self, now: Instant, maximum: Duration) -> Duration {
        let wait = self.ack_deadline.map_or(maximum, |deadline| {
            maximum.min(deadline.saturating_duration_since(now))
        });
        if self.staged_submitted {
            wait
        } else {
            self.staging
                .as_ref()
                .map_or(wait, |staging| staging.wait(now, wait))
        }
    }

    pub fn take_delivery(&mut self) -> Option<OutputFileSubmission> {
        self.delivery.take()
    }

    pub fn admission(&self) -> &OutputFileAdmission {
        &self.admission
    }

    fn live(&mut self) -> Result<(), Errno> {
        self.expire(Instant::now());
        if self.revoked {
            Err(Errno::ESTALE)
        } else {
            Ok(())
        }
    }

    fn start_ack_clock(&mut self) {
        if self.admission.journal().position().records != 0 && self.ack_deadline.is_none() {
            self.ack_deadline = Some(
                Instant::now()
                    + Duration::from_millis(self.limits.ack_progress_timeout_millis.into()),
            );
        }
    }

    /// Retain at most one unacknowledged publication and one older open pin.
    /// Backpressure does not replace the snapshot used for domain admission.
    pub fn publish(&mut self, snapshot: &OutputAuthoritySnapshot) -> Result<u64, Errno> {
        self.live()?;
        self.admission
            .connection()
            .require_observe()
            .map_err(|_| Errno::EACCES)?;
        if self.publication.is_some()
            || self
                .topology_open
                .is_some_and(|qid| qid != self.topology.qid)
        {
            return Err(Errno::EAGAIN);
        }
        let bytes = topology_bytes(self.epoch, snapshot)?;
        let qid = self.qids.allocate(1)?;
        let sequence = self.admission.publish(OutputFilePublication {
            topology_epoch: snapshot.topology_epoch,
            qid_path: qid,
        })?;
        self.topology = Arc::new(Topology {
            qid,
            bytes,
            snapshot: snapshot.clone(),
        });
        self.topology_read = false;
        self.publication = Some(Publication {
            sequence,
            qid,
            read: false,
        });
        self.start_ack_clock();
        Ok(qid)
    }

    pub fn settle(
        &mut self,
        transaction: TransactionId,
        outcome: OutputV1Outcome,
    ) -> Result<Option<AdmittedOutputProposal>, Errno> {
        self.live()?;
        let promoted = self.admission.settle(transaction, outcome)?;
        self.start_ack_clock();
        Ok(promoted)
    }

    fn submit(&mut self, data: &[u8]) -> Result<(), Errno> {
        self.live()?;
        let submit = decode_output_file_submit(data).map_err(|_| Errno::EINVAL)?;
        if submit.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        let staging = self.staging.as_ref().ok_or(Errno::EINVAL)?;
        if staging.bytes.len() != submit.candidate_bytes as usize {
            return Err(Errno::EINVAL);
        }
        let candidate = decode_output_file_record(&staging.bytes, OutputFileClass::Candidate)
            .map_err(|_| Errno::EINVAL)?;
        if candidate.header.submission_id != submit.submission_id
            || candidate.header.connection_epoch != submit.connection_epoch
        {
            return Err(Errno::EINVAL);
        }
        // Only one bounded handoff to the worker. Exact repeats of the
        // already-submitted staging handle still succeed while it is pending.
        if self.delivery.is_some()
            && self
                .admission
                .last_submission_id()
                .is_none_or(|last| submit.submission_id > last)
        {
            return Err(Errno::EAGAIN);
        }
        let receipt = self.admission.journal().position().next_sequence;
        let result = self.admission.submit(
            &staging.bytes,
            &self.topology.snapshot,
            OutputFilePublication {
                topology_epoch: self.topology.snapshot.topology_epoch,
                qid_path: self.topology.qid,
            },
        )?;
        self.staged_submitted = true;
        match &result {
            OutputFileSubmission::Replayed => return Ok(()),
            OutputFileSubmission::Negotiated(_) => {
                self.publication = Some(Publication {
                    sequence: self.admission.journal().position().next_sequence - 1,
                    qid: self.topology.qid,
                    read: self.topology_read,
                });
            }
            OutputFileSubmission::Refused(_) => self.refused = true,
            _ => {}
        }
        self.receipt = Some(receipt);
        self.delivery = Some(result);
        self.start_ack_clock();
        Ok(())
    }

    fn acknowledge(&mut self, data: &[u8]) -> Result<(), Errno> {
        let ack = decode_output_file_ack(data).map_err(|_| Errno::EINVAL)?;
        if ack.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        let bytes = self
            .reads
            .prepare_ack(self.admission.journal(), ack.sequence)?;
        if self
            .publication
            .as_ref()
            .is_some_and(|p| p.sequence <= ack.sequence && !p.read)
        {
            return Err(Errno::EAGAIN);
        }
        if self.admission.acknowledge(ack)? {
            self.reads.commit_ack(ack.sequence, bytes);
            if self.receipt.is_some_and(|receipt| receipt <= ack.sequence) {
                self.receipt = None;
            }
            if self
                .publication
                .as_ref()
                .is_some_and(|p| p.sequence <= ack.sequence)
            {
                self.publication = None;
            }
            self.ack_deadline = None;
            self.start_ack_clock();
            if self.refused && self.admission.journal().position().records == 0 {
                self.revoke();
            }
        }
        Ok(())
    }
}

fn object(epoch: u64, kind: OutputFileKind, body: &[u8]) -> Result<Vec<u8>, Errno> {
    encode_output_file_record(
        OutputFileHeader {
            kind,
            connection_epoch: epoch,
            submission_id: 0,
            sequence: 0,
        },
        body,
    )
    .map_err(|_| Errno::EINVAL)
}

fn topology_bytes(epoch: u64, snapshot: &OutputAuthoritySnapshot) -> Result<Vec<u8>, Errno> {
    let body = encode_output_file_topology(&OutputV1Snapshot {
        connection_epoch: epoch,
        snapshot: snapshot.clone(),
    })
    .map_err(|_| Errno::EINVAL)?;
    object(epoch, OutputFileKind::Topology, &body)
}
