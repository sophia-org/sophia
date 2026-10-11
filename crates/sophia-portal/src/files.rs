//! Portal file custody on the shared 9P core. The owner admits a trusted
//! requester; neither attach strings nor request records confer authority.
//!
//! Keep this owner across transport replacements: history, Qids and issued
//! executor custody outlive a connection. The owner must call `disconnect`
//! when its admitted transport ends, and service `expire` while it is idle.
//! An upload writer stays open through accepted PayloadEnd: clunk before End
//! revokes the grant even when all declared bytes have arrived.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use sophia_9p::Errno;
use sophia_9p::connection::ConnectionId;
use sophia_9p::export::PeerCredentials;
use sophia_9p::journal::{Journal, JournalBounds, Staging};
use sophia_protocol::portal_files::*;
use sophia_protocol::{
    NamespaceContext, NamespaceId, PortalBrokerResponseDecision, PortalBrokerResponsePacket,
    PortalGrant, PortalGrantState, PortalRequest, PortalTransferId, PortalTransferKind,
};

use crate::{PortalPolicyDecision, PortalRequestGrantLifecycle};

mod filesystem;
mod submission;
pub use filesystem::{PortalFileHandle, PortalFileNode};

const EBUSY: Errno = Errno(16);
const EALREADY: Errno = Errno(114);

/// Trusted owner facts, never decoded from a requester record. Implementations
/// consult live namespace authority, and make a policy decision without payload.
pub trait PortalFileAuthority {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext>;
    /// Correlate the retained frontend request with its actual current source.
    /// A namespace alone is insufficient: PRIMARY and CLIPBOARD have separate
    /// owners and generations. A requester-supplied generation is not evidence.
    fn source_generation(&self, request: &PortalRequest) -> Option<u64>;
    /// Nondecreasing owner-clock milliseconds, shared with request deadlines.
    fn now_msec(&self) -> u64;
    fn live(&self) -> bool;
    /// Changes on a security transition, including lock followed by unlock.
    /// Restoring liveness must never restore grants from an earlier interval.
    fn revision(&self) -> u64 {
        1
    }
    fn allow(&self, _request: &PortalRequest) -> bool {
        false
    }
    fn payload_handler(&self) -> &dyn crate::handlers::PortalPayloadHandler {
        &crate::handlers::ClipboardPayloadHandler
    }
}

/// Evidence selected by the trusted owner, never by an attach or file write.
#[derive(Clone, Copy, Debug)]
pub enum PortalRequesterCustody {
    ProtectedPeer(PeerCredentials),
    /// Session registered the in-process authority before creating this
    /// connection. Kernel credentials do not prove this custody.
    /// The owner must not use this variant for accepted path connections.
    SessionAuthority {
        registration: std::num::NonZeroU64,
    },
}

/// Supplied only after transport custody admission. A fresh epoch is required
/// on every reconnect; a pair cannot be widened by an attach or a candidate.
#[derive(Clone, Copy, Debug)]
pub struct PortalRequesterAdmission {
    pub connection: ConnectionId,
    pub custody: PortalRequesterCustody,
    pub epoch: u64,
    pub source: NamespaceId,
    pub target: NamespaceId,
}

/// One payload whose End took execution custody. The decision owner never
/// executes it. The recipient must recheck its own source/recipient identity
/// and report the actual outcome through `settle_execution` exactly once.
pub struct PortalFileExecution {
    pub request: PortalRequest,
    pub grant: PortalGrant,
    pub payload: Vec<u8>,
    pub authority_revision: u64,
}

struct Upload {
    transfer: PortalTransferId,
    qid: u64,
    total: usize,
    bytes: Vec<u8>,
    writer: bool,
}

struct Accepted {
    id: u64,
    bytes: Vec<u8>,
    receipt: u64,
}

struct Connection {
    admission: PortalRequesterAdmission,
    authority_revision: u64,
    base: u64,
    attached: bool,
    revoked: bool,
    negotiated: bool,
    refused: bool,
    journal: Journal,
    ack_deadline: Option<Instant>,
    refusal_deadline: Option<Instant>,
    accepted: Option<Accepted>,
    acknowledged: u64,
    staging: Option<Staging>,
    submitted: bool,
    terminal: BTreeMap<PortalTransferId, ()>,
}

pub struct PortalFileExport<A> {
    authority: A,
    limits: PortalFileLimits,
    lifecycle: PortalRequestGrantLifecycle,
    retained: usize,
    next_qid: u64,
    last_epoch: u64,
    connection: Option<Connection>,
    upload: Option<Upload>,
    execution: Option<PortalFileExecution>,
    issued: Option<PortalTransferId>,
    faulted: bool,
}

impl<A: PortalFileAuthority> PortalFileExport<A> {
    pub fn new(authority: A, limits: PortalFileLimits) -> Result<Self, Errno> {
        limits.validate().map_err(|_| Errno::EINVAL)?;
        Ok(Self {
            authority,
            lifecycle: PortalRequestGrantLifecycle::with_capacity(
                limits.broker_generation,
                limits.max_transfers as usize,
            )
            .map_err(|_| Errno::EINVAL)?,
            limits,
            retained: 0,
            next_qid: 1,
            last_epoch: 0,
            connection: None,
            upload: None,
            execution: None,
            issued: None,
            faulted: false,
        })
    }

    pub fn authority(&self) -> &A {
        &self.authority
    }
    pub fn authority_mut(&mut self) -> &mut A {
        &mut self.authority
    }

    pub fn admit(&mut self, admission: PortalRequesterAdmission) -> Result<(), Errno> {
        if self.faulted {
            return Err(Errno::EIO);
        }
        if self.connection.is_some() || self.issued.is_some() {
            return Err(EBUSY);
        }
        if !self.authority.live()
            || admission.epoch <= self.last_epoch
            || !admission.source.is_valid()
            || !admission.target.is_valid()
            || admission.source == admission.target
            || self.authority.namespace(admission.source).is_none()
            || self.authority.namespace(admission.target).is_none()
        {
            return Err(Errno::EACCES);
        }
        let base = self.allocate_qids(9)?;
        self.last_epoch = admission.epoch;
        self.connection = Some(Connection {
            admission,
            authority_revision: self.authority.revision(),
            base,
            attached: false,
            revoked: false,
            negotiated: false,
            refused: false,
            journal: Journal::new(admission.epoch),
            ack_deadline: None,
            refusal_deadline: None,
            accepted: None,
            acknowledged: 0,
            staging: None,
            submitted: false,
            terminal: BTreeMap::new(),
        });
        Ok(())
    }

    /// Closing the transport ends unissued grants, but cannot undo an issued
    /// effect or admit a replacement beside its retained payload.
    pub fn disconnect(&mut self, connection: ConnectionId) {
        if self
            .connection
            .as_ref()
            .is_none_or(|c| c.admission.connection != connection)
        {
            return;
        }
        self.revoke();
        self.connection = None;
    }

    pub fn revoke(&mut self) {
        let transfers: Vec<_> = self
            .connection
            .as_ref()
            .map(|c| c.terminal.keys().copied().collect())
            .unwrap_or_default();
        for id in transfers {
            if self.issued != Some(id) {
                self.finish_reserved(id, PortalGrantState::Revoked);
            }
        }
        self.upload = None;
        if let Some(c) = &mut self.connection {
            c.revoked = true;
            c.staging = None;
        }
    }

    pub fn expire(&mut self, now: Instant) {
        if self.connection.as_ref().is_some_and(|c| {
            c.authority_revision != self.authority.revision()
                || c.ack_deadline.is_some_and(|d| now >= d)
                || c.refusal_deadline.is_some_and(|d| now >= d)
                || self.authority.namespace(c.admission.source).is_none()
                || self.authority.namespace(c.admission.target).is_none()
        }) || !self.authority.live()
        {
            self.revoke();
            return;
        }
        let transfers: Vec<_> = self
            .connection
            .as_ref()
            .map(|c| c.terminal.keys().copied().collect())
            .unwrap_or_default();
        for id in transfers {
            if self.issued == Some(id) {
                continue;
            }
            if let Some(grant) = self.lifecycle.grant(id) {
                let state = if grant.deadline_msec <= self.authority.now_msec() {
                    Some(PortalGrantState::Expired)
                } else if !self.permissions(
                    grant.source_namespace,
                    grant.target_namespace,
                    grant.kind,
                ) || !self.current_source(id)
                {
                    Some(PortalGrantState::Revoked)
                } else {
                    None
                };
                if let Some(state) = state {
                    self.finish_reserved(id, state);
                }
            }
        }
        if let Some(c) = &mut self.connection
            && !c.submitted
            && c.staging.as_ref().is_some_and(|s| s.expired(now))
        {
            c.staging = None;
        }
    }

    pub fn take_execution(&mut self) -> Option<PortalFileExecution> {
        // Work queued by End has not yet reached its recipient. Recheck at
        // handoff as well as at End; the recipient owns the final effect check.
        self.expire(Instant::now());
        let work = self.execution.take()?;
        let expired = work.grant.deadline_msec <= self.authority.now_msec();
        if !self.authority.live()
            || expired
            || !self.permissions(
                work.grant.source_namespace,
                work.grant.target_namespace,
                work.grant.kind,
            )
            || self.connection.as_ref().is_none_or(|c| c.revoked)
            || !self.current_source(work.grant.transfer)
            || self.faulted
        {
            self.finish_reserved(
                work.grant.transfer,
                if expired {
                    PortalGrantState::Expired
                } else {
                    PortalGrantState::Revoked
                },
            );
            None
        } else {
            self.issued = Some(work.grant.transfer);
            Some(work)
        }
    }

    pub fn settle_execution(
        &mut self,
        transfer: PortalTransferId,
        succeeded: bool,
    ) -> Result<(), Errno> {
        if self.issued != Some(transfer) || self.execution.is_some() {
            return Err(EALREADY);
        }
        self.finish(
            transfer,
            if succeeded {
                PortalGrantState::Completed
            } else {
                PortalGrantState::Revoked
            },
        )?;
        self.issued = None;
        Ok(())
    }

    fn permissions(
        &self,
        source: NamespaceId,
        target: NamespaceId,
        kind: PortalTransferKind,
    ) -> bool {
        self.authority
            .namespace(source)
            .is_some_and(|n| n.capabilities.allows_publish(kind.capability()))
            && self
                .authority
                .namespace(target)
                .is_some_and(|n| n.capabilities.allows_request(kind.capability()))
    }

    fn allocate_qids(&mut self, count: u64) -> Result<u64, Errno> {
        let first = self.next_qid;
        self.next_qid = first.checked_add(count).ok_or(Errno::ENOSPC)?;
        Ok(first)
    }

    fn live(&mut self) -> Result<&mut Connection, Errno> {
        self.expire(Instant::now());
        if self.faulted {
            return Err(Errno::EIO);
        }
        self.connection
            .as_mut()
            .filter(|c| !c.revoked)
            .ok_or(Errno::ESTALE)
    }

    fn start_ack_clock(&mut self) {
        if let Some(c) = &mut self.connection
            && c.journal.position().records != 0
            && c.ack_deadline.is_none()
        {
            c.ack_deadline = Some(
                Instant::now() + Duration::from_millis(self.limits.ack_progress_timeout_ms.into()),
            );
        }
    }

    fn bounds(&self, terminal: usize) -> JournalBounds {
        JournalBounds {
            records: (self.limits.journal_records as usize).saturating_sub(terminal),
            bytes: (self.limits.journal_bytes as usize).saturating_sub(terminal * 48),
        }
    }

    fn finish(&mut self, id: PortalTransferId, state: PortalGrantState) -> Result<(), Errno> {
        let retained = self
            .connection
            .as_ref()
            .is_some_and(|c| c.terminal.contains_key(&id));
        if retained {
            let count = self.connection.as_ref().unwrap().terminal.len() - 1;
            let bounds = self.bounds(count);
            let c = self.connection.as_mut().unwrap();
            let event = encode_portal_file_event(
                c.admission.epoch,
                c.journal.next_sequence(),
                &PortalFileEvent::TransferOutcome(PortalTransferOutcome {
                    transfer: id,
                    state,
                }),
            )
            .map_err(|_| Errno::EINVAL)?;
            // This spends a credit reserved before the grant was created.
            let prepared = c.journal.prepare(event, bounds)?;
            match state {
                PortalGrantState::Completed => self.lifecycle.complete(id),
                PortalGrantState::Revoked => self.lifecycle.executor_failed(id),
                PortalGrantState::Expired => self
                    .lifecycle
                    .expire_transfer(id, self.authority.now_msec()),
                PortalGrantState::Active => return Err(Errno::EINVAL),
            }
            .map_err(|_| EALREADY)?;
            prepared.commit();
            c.terminal.remove(&id);
            self.start_ack_clock();
        } else {
            match state {
                PortalGrantState::Completed => self.lifecycle.complete(id),
                _ => self.lifecycle.executor_failed(id),
            }
            .map_err(|_| EALREADY)?;
        }
        if self.upload.as_ref().is_some_and(|u| u.transfer == id) {
            self.upload = None;
        }
        if self
            .execution
            .as_ref()
            .is_some_and(|e| e.grant.transfer == id)
        {
            self.execution = None;
        }
        Ok(())
    }

    fn current_source(&self, id: PortalTransferId) -> bool {
        self.lifecycle
            .request(id)
            .is_some_and(|r| self.authority.source_generation(r) == Some(r.transfer.generation))
    }

    fn execution_pending(&self, id: PortalTransferId) -> bool {
        self.issued == Some(id)
            || self
                .execution
                .as_ref()
                .is_some_and(|e| e.grant.transfer == id)
    }

    /// A terminal credit failure is an owner invariant failure. Fence all
    /// future work, including reconnect, rather than silently leaking a grant.
    fn finish_reserved(&mut self, id: PortalTransferId, state: PortalGrantState) {
        if self.finish(id, state).is_err() {
            self.faulted = true;
            self.execution = None;
            self.upload = None;
            if let Some(c) = &mut self.connection {
                c.revoked = true;
            }
        }
    }
}

fn codec(error: PortalFileError) -> Errno {
    match error {
        PortalFileError::StaleEpoch => Errno::ESTALE,
        PortalFileError::Invalid(_) => Errno::EINVAL,
    }
}
