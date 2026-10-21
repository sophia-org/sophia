use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use sophia_protocol::*;

#[cfg(feature = "experimental-shell-capture")]
mod capture;
#[cfg(feature = "experimental-shell-capture")]
pub use capture::*;

/// Recoverable protocol outcomes; callers must not turn these into session loss.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentStoreError {
    Stale,
    Budget,
    Malformed,
    Incomplete,
    Revoked,
    ClockRegression,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContentMemoryUsage {
    pub staging: u64,
    pub resident: u64,
    pub retiring: u64,
    pub reserved_resident: u64,
    /// Conservative compositor upload/backing credit retained with this
    /// resource generation. It is known storage capacity, not driver VRAM.
    pub backing: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentResourceEvent {
    pub transaction: TransactionId,
    pub record: ShellContentRecord,
}

struct Transfer {
    description: ContentResourceBegin,
    layout: ContentResourceLayout,
    transaction: TransactionId,
    deadline: u64,
    // A stopped worker retains its mapping and charge until completion.
    stopped: Option<(ContentReason, TransactionId)>,
    #[cfg(feature = "experimental-shell-capture")]
    capture: Option<Arc<()>>,
}

mod backing;
#[cfg(feature = "experimental-shell-capture")]
pub use backing::ContentCapturedImage;
pub use backing::{ContentCpuResourceLease, ContentResourceLease};

struct Accepted {
    pixels: ContentResourceLease,
    retiring: bool,
    transaction: TransactionId,
    release_pending: bool,
    #[cfg(feature = "experimental-shell-capture")]
    capture_evicted: bool,
}

impl Accepted {
    fn releasable(&self) -> bool {
        #[cfg(feature = "experimental-shell-capture")]
        if !self.capture_evicted {
            return false;
        }
        self.pixels.custody_is_exclusive()
    }
}

pub struct ContentResourceStore {
    limits: ContentLimits,
    transfers: BTreeMap<ContentResourceId, Transfer>,
    accepted: BTreeMap<ContentResourceId, Accepted>,
    high_water: BTreeMap<u64, u64>,
    greatest_id: u64,
    usage: ContentMemoryUsage,
    events: VecDeque<ContentResourceEvent>,
    /// Terminal transfer and resource-release credits, excluding queued events.
    response_credits: usize,
    last_now: u64,
    revoked: bool,
}

impl ContentResourceStore {
    pub(crate) fn add_accounting(&self, value: &mut super::ContentEpochAccounting) {
        value.transfers += self.transfers.len();
        value.resources += self.accepted.len();
        value.resource_ids += self.high_water.len();
        value.memory.staging += self.usage.staging;
        value.memory.resident += self.usage.resident;
        value.memory.retiring += self.usage.retiring;
        value.memory.reserved_resident += self.usage.reserved_resident;
        value.memory.backing += self.usage.backing;
        value.response_records += self.control_occupancy();
    }

    pub(crate) fn additional_response_credit(&self, record: &ShellContentRecord) -> usize {
        match record {
            ShellContentRecord::ResourceCancel(value) => {
                usize::from(!self.transfers.contains_key(&value.resource))
            }
            ShellContentRecord::ResourceRetire(value) => usize::from(
                self.accepted
                    .get(&value.resource)
                    .is_none_or(|resource| resource.retiring),
            ),
            _ => 1,
        }
    }

    pub(crate) fn control_occupancy(&self) -> usize {
        self.events.len() + self.response_credits
    }

    pub fn new(limits: ContentLimits) -> Result<Self, ContentStoreError> {
        limits
            .validate()
            .map_err(|_| ContentStoreError::Malformed)?;
        Ok(Self {
            limits,
            transfers: BTreeMap::new(),
            accepted: BTreeMap::new(),
            high_water: BTreeMap::new(),
            greatest_id: 0,
            usage: ContentMemoryUsage::default(),
            events: VecDeque::new(),
            response_credits: 0,
            last_now: 0,
            revoked: false,
        })
    }

    pub fn usage(&self) -> ContentMemoryUsage {
        self.usage
    }
    pub fn grant(&self) -> ContentGrant {
        self.limits.grant
    }
    #[cfg(feature = "experimental-owned-copy")]
    pub(super) fn is_revoked(&self) -> bool {
        self.revoked
    }
    pub fn quiescent(&self) -> bool {
        self.transfers.is_empty() && self.accepted.is_empty()
    }
    pub fn take_event(&mut self) -> Option<ContentResourceEvent> {
        self.events.pop_front()
    }
    // This event queue contains authoritative lifecycle events, not replies
    // synthesized for failed client controls. ResourceStatus entries are final
    // transfer settlements; the shell outbox preserves that provenance.
    pub fn pending_event(&self) -> Option<&ContentResourceEvent> {
        self.events.front()
    }

    fn check(
        &self,
        grant: ContentGrant,
        transaction: TransactionId,
    ) -> Result<(), ContentStoreError> {
        if self.revoked {
            return Err(ContentStoreError::Revoked);
        }
        if grant != self.limits.grant || !transaction.is_valid() {
            return Err(ContentStoreError::Stale);
        }
        Ok(())
    }

    fn time(&mut self, now: u64) -> Result<(), ContentStoreError> {
        if now < self.last_now {
            return Err(ContentStoreError::ClockRegression);
        }
        self.last_now = now;
        Ok(())
    }

    fn status(
        &mut self,
        transaction: TransactionId,
        resource: ContentResourceId,
        status: u16,
        reason: ContentReason,
        admitted_bytes: u64,
    ) {
        self.events.push_back(ContentResourceEvent {
            transaction,
            record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
                grant: self.limits.grant,
                resource,
                status,
                reason: reason as u16,
                next_ordinal: 0,
                admitted_bytes,
            }),
        });
    }

    /// Reserve bounded custody for a sealed source. Every successful reservation
    /// needs complete_isb, even if no worker starts; stopping cannot release a
    /// worker's mapping or charge before that handoff.
    pub fn begin_isb(
        &mut self,
        transaction: TransactionId,
        description: ContentResourceBegin,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        self.check(description.grant, transaction)?;
        self.time(now)?;
        let key = description.resource;
        if key.id == 0 || key.generation == 0 {
            return Err(ContentStoreError::Malformed);
        }
        let fresh = match self.high_water.get(&key.id) {
            Some(generation) => generation.checked_add(1) == Some(key.generation),
            None => key.id > self.greatest_id && key.generation == 1,
        };
        if !fresh {
            return Err(ContentStoreError::Stale);
        }
        if !self.high_water.contains_key(&key.id)
            && self.high_water.len() >= self.limits.max_resource_ids as usize
        {
            return Err(ContentStoreError::Budget);
        }
        let layout = description
            .layout(&self.limits)
            .map_err(|_| ContentStoreError::Malformed)?;
        let bytes = layout.total_bytes;
        if self.transfers.len() >= self.limits.max_open_transfers as usize
            || self.transfers.len() + self.accepted.len() >= self.limits.max_live_resources as usize
            || self.usage.staging + bytes > self.limits.max_staging_bytes
            || self.usage.resident + self.usage.reserved_resident + bytes
                > self.limits.max_resident_bytes
            || self.usage.backing + bytes
                > self.limits.max_resident_bytes + self.limits.max_retiring_bytes
            || self.events.len() + self.response_credits + 2
                > self.limits.max_control_records as usize
        {
            return Err(ContentStoreError::Budget);
        }
        let deadline = now
            .checked_add(u64::from(self.limits.transfer_timeout_ms))
            .ok_or(ContentStoreError::Malformed)?;
        // Refused capacity/layout checks do not consume the generation.
        self.greatest_id = self.greatest_id.max(key.id);
        self.high_water.insert(key.id, key.generation);
        self.usage.staging += bytes;
        self.usage.reserved_resident += bytes;
        self.usage.backing += bytes;
        self.response_credits += 2;
        self.transfers.insert(
            key,
            Transfer {
                description,
                layout,
                transaction,
                deadline,
                stopped: None,
                #[cfg(feature = "experimental-shell-capture")]
                capture: None,
            },
        );
        Ok(())
    }

    /// Reconciles a finished validation job (or an offer for which no job was
    /// launched). It must run even after cancellation or epoch revocation.
    /// Only this handoff lets a stopped transfer release its retained charge.
    /// The result is taken only after the grant, clock and transfer match.
    /// If it remains Some on return, the caller still owns the completion and
    /// must retry or route it to its actual owner; an error alone is not custody.
    pub fn complete_isb(
        &mut self,
        grant: ContentGrant,
        resource: ContentResourceId,
        result: &mut Option<Result<super::ValidatedIsbPixels, ContentStoreError>>,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        if grant != self.limits.grant || result.is_none() {
            return Err(ContentStoreError::Stale);
        }
        self.expire(now)?;
        let transfer = self
            .transfers
            .get(&resource)
            .ok_or(ContentStoreError::Stale)?;
        let stopped = transfer.stopped;
        // A CPU validator cannot settle a renderer-owned source, even with an
        // identically sized mapping or a failure result.
        #[cfg(feature = "experimental-shell-capture")]
        if transfer.capture.is_some() {
            return Err(ContentStoreError::Stale);
        }
        let result = result.take().expect("completion checked");
        if let Some((reason, transaction)) = stopped {
            // The result and its mapping drop only after the worker returned.
            self.abort_finished(resource, reason, Some(transaction));
            return Err(if reason == ContentReason::Revoked {
                ContentStoreError::Revoked
            } else {
                ContentStoreError::Stale
            });
        }
        let pixels = match result {
            Ok(pixels) if pixels.bytes().len() as u64 == transfer.layout.total_bytes => pixels,
            _ => {
                self.abort_finished(resource, ContentReason::Malformed, None);
                return Err(ContentStoreError::Malformed);
            }
        };
        let transfer = self
            .transfers
            .remove(&resource)
            .expect("checked sealed transfer");
        let bytes = transfer.layout.total_bytes;
        self.usage.staging -= bytes;
        self.usage.reserved_resident -= bytes;
        self.usage.resident += bytes;
        self.response_credits -= 1;
        self.accepted.insert(
            resource,
            Accepted {
                pixels: ContentResourceLease::cpu(transfer.description, pixels.0),
                retiring: false,
                transaction: transfer.transaction,
                release_pending: false,
                #[cfg(feature = "experimental-shell-capture")]
                capture_evicted: true,
            },
        );
        self.status(
            transfer.transaction,
            resource,
            2,
            ContentReason::None,
            bytes,
        );
        Ok(())
    }

    pub fn cancel(
        &mut self,
        transaction: TransactionId,
        cancel: &ContentResourceCancel,
    ) -> Result<(), ContentStoreError> {
        self.check(cancel.grant, transaction)?;
        if !self.transfers.contains_key(&cancel.resource) {
            return Err(ContentStoreError::Stale);
        }
        self.abort(cancel.resource, ContentReason::Cancelled, Some(transaction));
        Ok(())
    }

    /// The descriptor owner withdraws interest without releasing validation
    /// custody. This also works on a revoked store retained for worker cleanup.
    pub(super) fn stop_isb(
        &mut self,
        resource: ContentResourceId,
        reason: ContentReason,
        now: u64,
    ) -> Result<ContentReason, ContentStoreError> {
        self.expire(now)?;
        let transfer = self
            .transfers
            .get_mut(&resource)
            .ok_or(ContentStoreError::Stale)?;
        Ok(transfer
            .stopped
            .get_or_insert((reason, transfer.transaction))
            .0)
    }

    pub(super) fn isb_stop_reason(&self, resource: ContentResourceId) -> Option<ContentReason> {
        self.transfers
            .get(&resource)?
            .stopped
            .map(|(reason, _)| reason)
    }

    fn abort(
        &mut self,
        key: ContentResourceId,
        reason: ContentReason,
        request: Option<TransactionId>,
    ) {
        if let Some(transfer) = self.transfers.get_mut(&key) {
            transfer
                .stopped
                .get_or_insert((reason, request.unwrap_or(transfer.transaction)));
        }
    }

    fn abort_finished(
        &mut self,
        key: ContentResourceId,
        reason: ContentReason,
        request: Option<TransactionId>,
    ) {
        if let Some(transfer) = self.transfers.remove(&key) {
            self.usage.staging -= transfer.layout.total_bytes;
            self.usage.reserved_resident -= transfer.layout.total_bytes;
            self.usage.backing -= transfer.layout.total_bytes;
            self.response_credits -= 2;
            self.status(
                request.unwrap_or(transfer.transaction),
                key,
                if reason == ContentReason::Cancelled {
                    4
                } else {
                    3
                },
                reason,
                0,
            );
        }
    }

    pub fn expire(&mut self, now: u64) -> Result<(), ContentStoreError> {
        self.time(now)?;
        let expired: Vec<_> = self
            .transfers
            .iter()
            .filter(|(_, t)| now >= t.deadline)
            .map(|(key, _)| *key)
            .collect();
        for key in expired {
            self.abort(key, ContentReason::Timeout, None);
        }
        Ok(())
    }

    /// Only an accepted generation can be referenced. A retiring generation
    /// remains readable through old leases but cannot acquire a new consumer.
    pub fn lease(
        &self,
        grant: ContentGrant,
        resource: ContentResourceId,
    ) -> Result<ContentResourceLease, ContentStoreError> {
        if self.revoked {
            return Err(ContentStoreError::Revoked);
        }
        if grant != self.limits.grant {
            return Err(ContentStoreError::Stale);
        }
        let value = self
            .accepted
            .get(&resource)
            .ok_or(ContentStoreError::Stale)?;
        if value.retiring {
            return Err(ContentStoreError::Stale);
        }
        Ok(value.pixels.clone())
    }

    pub fn retire(
        &mut self,
        transaction: TransactionId,
        request: &ContentResourceRetire,
    ) -> Result<(), ContentStoreError> {
        self.check(request.grant, transaction)?;
        let value = self
            .accepted
            .get_mut(&request.resource)
            .ok_or(ContentStoreError::Stale)?;
        if value.retiring {
            return Err(ContentStoreError::Stale);
        }
        let bytes = value.pixels.description().total_bytes;
        if self.usage.retiring + bytes > self.limits.max_retiring_bytes {
            return Err(ContentStoreError::Budget);
        }
        value.retiring = true;
        value.transaction = transaction;
        self.usage.resident -= bytes;
        self.usage.retiring += bytes;
        self.collect();
        Ok(())
    }

    pub fn collect(&mut self) {
        let ready: Vec<_> = self
            .accepted
            .iter()
            .filter(|(_, v)| !v.release_pending && (v.retiring || self.revoked) && v.releasable())
            .map(|(key, _)| *key)
            .collect();
        for resource in ready {
            let value = self.accepted.get_mut(&resource).expect("selected resource");
            let transaction = value.transaction;
            let defer = !self.revoked;
            if defer {
                // Keep the backing and its byte charge until ResourceReleased
                // enters the journal, not merely the outbound FIFO.
                value.release_pending = true;
            } else {
                self.remove_accepted(resource);
            }
            self.response_credits -= 1;
            self.events.push_back(ContentResourceEvent {
                transaction,
                record: ShellContentRecord::ResourceReleased(ContentResourceReleased {
                    grant: self.limits.grant,
                    resource,
                    reason: if self.revoked {
                        ContentReason::Revoked
                    } else {
                        ContentReason::None
                    } as u16,
                }),
            });
        }
        if self.revoked {
            let pending: Vec<_> = self
                .accepted
                .iter()
                .filter(|(_, v)| v.release_pending && v.releasable())
                .map(|(key, _)| *key)
                .collect();
            for key in pending {
                self.remove_accepted(key);
            }
        }
    }

    fn remove_accepted(&mut self, resource: ContentResourceId) {
        let value = self.accepted.remove(&resource).expect("selected resource");
        let bytes = value.pixels.description().total_bytes;
        if value.retiring {
            self.usage.retiring -= bytes;
        } else {
            self.usage.resident -= bytes;
        }
        self.usage.backing -= bytes;
    }

    /// Called by the image owner only after the corresponding ResourceReleased
    /// record has entered the live journal. Queueing the event is insufficient.
    /// Epoch revocation instead drops this obligation with the dead journal.
    pub fn confirm_isb_release(
        &mut self,
        resource: ContentResourceId,
    ) -> Result<(), ContentStoreError> {
        let value = self
            .accepted
            .get(&resource)
            .ok_or(ContentStoreError::Stale)?;
        if !value.release_pending
            || !value.releasable()
            || self.events.iter().any(|event| {
                matches!(&event.record,
                ShellContentRecord::ResourceReleased(released) if released.resource == resource)
            })
        {
            return Err(ContentStoreError::Incomplete);
        }
        self.remove_accepted(resource);
        Ok(())
    }

    pub(crate) fn isb_release_pending(&self, resource: ContentResourceId) -> bool {
        self.accepted
            .get(&resource)
            .is_some_and(|value| value.release_pending)
    }

    /// Revoke input/candidate admission immediately. Storage still referenced by
    /// a consumer is retained; the session keeps this owner in its retired pool.
    pub fn revoke(&mut self) {
        self.revoked = true;
        let incomplete: Vec<_> = self.transfers.keys().copied().collect();
        for key in incomplete {
            self.abort(key, ContentReason::Revoked, None);
        }
        // Do not force resident storage through the active retiring ceiling.
        // The retired-epoch owner accounts the whole remaining footprint against
        // its pre-reserved global credit, retaining these disjoint local classes.
        self.collect();
    }
}
