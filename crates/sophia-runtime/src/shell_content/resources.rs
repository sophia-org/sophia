use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use sophia_protocol::*;

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
    bytes: Vec<u8>,
    ordinal: u32,
    deadline: u64,
    idle_deadline: u64,
}

struct Pixels {
    description: ContentResourceBegin,
    bytes: Arc<Vec<u8>>,
}

struct Accepted {
    pixels: Arc<Pixels>,
    retiring: bool,
    transaction: TransactionId,
}

/// An immutable reference to accepted pixels. Clones are real storage consumers.
/// Drop the last renderer/upload reference before asking the owner to collect.
#[derive(Clone)]
pub struct ContentResourceLease(Arc<Pixels>);

impl ContentResourceLease {
    pub fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }
    pub fn description(&self) -> &ContentResourceBegin {
        &self.0.description
    }

    /// Shares the immutable accepted allocation with a renderer frame.
    /// This is a refcount bump: no second pixel allocation escapes the
    /// connection's resident-resource accounting.
    pub fn shared_bytes(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.0.bytes)
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
    pub fn quiescent(&self) -> bool {
        self.transfers.is_empty() && self.accepted.is_empty()
    }
    pub fn take_event(&mut self) -> Option<ContentResourceEvent> {
        self.events.pop_front()
    }
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

    /// Begin reserves staging, resident credit, a resource slot and all future
    /// response capacity. Failed Begins still consume a valid generation.
    pub fn begin(
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
        self.greatest_id = self.greatest_id.max(key.id);
        self.high_water.insert(key.id, key.generation);
        let layout = description
            .layout(&self.limits)
            .map_err(|_| ContentStoreError::Malformed)?;
        let bytes = layout.total_bytes;
        if self.transfers.len() >= self.limits.max_open_transfers as usize
            || self.transfers.len() + self.accepted.len() >= self.limits.max_live_resources as usize
            || self.usage.staging + bytes > self.limits.max_staging_bytes
            || self.usage.resident + self.usage.reserved_resident + bytes
                > self.limits.max_resident_bytes
            || self.events.len() + self.response_credits + 3
                > self.limits.max_control_records as usize
        {
            return Err(ContentStoreError::Budget);
        }
        let deadline = now
            .checked_add(u64::from(self.limits.transfer_timeout_ms))
            .ok_or(ContentStoreError::Malformed)?;
        let idle_deadline = now
            .checked_add(u64::from(self.limits.transfer_idle_timeout_ms))
            .ok_or(ContentStoreError::Malformed)?;
        let mut storage = Vec::new();
        storage
            .try_reserve_exact(bytes as usize)
            .map_err(|_| ContentStoreError::Budget)?;
        self.usage.staging += bytes;
        self.usage.reserved_resident += bytes;
        self.response_credits += 2;
        self.transfers.insert(
            key,
            Transfer {
                description,
                layout,
                transaction,
                bytes: storage,
                ordinal: 0,
                deadline,
                idle_deadline,
            },
        );
        self.status(transaction, key, 1, ContentReason::None, bytes);
        Ok(())
    }

    /// Dense canonical chunks only. A malformed chunk terminates that transfer;
    /// it cannot leave a partial accepted resource or consume unbounded storage.
    pub fn chunk(
        &mut self,
        transaction: TransactionId,
        chunk: &ContentResourceChunk,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        self.check(chunk.grant, transaction)?;
        self.time(now)?;
        self.expire(now)?;
        let Some(transfer) = self.transfers.get_mut(&chunk.resource) else {
            return Err(ContentStoreError::Stale);
        };
        let remaining = transfer.layout.total_bytes - transfer.bytes.len() as u64;
        let expected = remaining
            .min(u64::from(transfer.layout.row_bytes) * u64::from(transfer.layout.rows_per_chunk));
        if chunk.ordinal != transfer.ordinal
            || chunk.offset != transfer.bytes.len() as u64
            || chunk.bytes.len() as u64 != expected
            || expected == 0
            || chunk
                .bytes
                .chunks_exact(4)
                .any(|p| p[0] > p[3] || p[1] > p[3] || p[2] > p[3])
        {
            self.abort(chunk.resource, ContentReason::Malformed, Some(transaction));
            return Err(ContentStoreError::Malformed);
        }
        transfer.bytes.extend_from_slice(&chunk.bytes);
        transfer.ordinal += 1;
        transfer.idle_deadline = now
            .saturating_add(u64::from(self.limits.transfer_idle_timeout_ms))
            .min(transfer.deadline);
        Ok(())
    }

    pub fn end(
        &mut self,
        transaction: TransactionId,
        end: &ContentResourceEnd,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        self.check(end.grant, transaction)?;
        self.time(now)?;
        self.expire(now)?;
        let Some(transfer) = self.transfers.get(&end.resource) else {
            return Err(ContentStoreError::Stale);
        };
        if end.total_bytes != transfer.layout.total_bytes
            || end.chunk_count != transfer.layout.chunk_count
            || transfer.bytes.len() as u64 != end.total_bytes
            || transfer.ordinal != end.chunk_count
        {
            self.abort(end.resource, ContentReason::Incomplete, Some(transaction));
            return Err(ContentStoreError::Incomplete);
        }
        let transfer = self
            .transfers
            .remove(&end.resource)
            .expect("checked transfer");
        let bytes = transfer.layout.total_bytes;
        self.usage.staging -= bytes;
        self.usage.reserved_resident -= bytes;
        self.usage.resident += bytes;
        self.response_credits -= 1;
        self.accepted.insert(
            end.resource,
            Accepted {
                pixels: Arc::new(Pixels {
                    description: transfer.description,
                    bytes: Arc::new(transfer.bytes),
                }),
                retiring: false,
                transaction,
            },
        );
        self.status(transaction, end.resource, 2, ContentReason::None, bytes);
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

    fn abort(
        &mut self,
        key: ContentResourceId,
        reason: ContentReason,
        request: Option<TransactionId>,
    ) {
        if let Some(transfer) = self.transfers.remove(&key) {
            self.usage.staging -= transfer.layout.total_bytes;
            self.usage.reserved_resident -= transfer.layout.total_bytes;
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
            .filter(|(_, t)| now >= t.deadline || now >= t.idle_deadline)
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
        Ok(ContentResourceLease(value.pixels.clone()))
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
        let bytes = value.pixels.description.total_bytes;
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
            .filter(|(_, v)| (v.retiring || self.revoked) && Arc::strong_count(&v.pixels) == 1)
            .map(|(key, _)| *key)
            .collect();
        for resource in ready {
            let value = self.accepted.remove(&resource).expect("selected resource");
            if value.retiring {
                self.usage.retiring -= value.pixels.description.total_bytes;
            } else {
                self.usage.resident -= value.pixels.description.total_bytes;
            }
            self.response_credits -= 1;
            self.events.push_back(ContentResourceEvent {
                transaction: value.transaction,
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
