//! A lock provider's resources: uploads through the fixed slots and the
//! images they become. Resources belong to the connection epoch, so a
//! provider may keep one across lock epochs. Every size is checked against
//! the epoch's limits before a byte is held.
use std::collections::BTreeMap;
use std::sync::Arc;

use sophia_9p::Errno;
use sophia_protocol::lock_files::*;

use super::reason;

/// One upload in progress, bound to its slot from Begin until End or Cancel.
struct Upload {
    /// Fences writers opened on an earlier binding of the same slot.
    binding: u64,
    transaction: u64,
    resource: LockResourceId,
    width_px: u32,
    height_px: u32,
    expected: u64,
    /// The image's final storage, taken at Begin and held only here until
    /// End: written in place at the cursor and handed on without another copy.
    pixels: Option<Arc<[u8]>>,
    written: u64,
}

/// A whole image the provider may name in a candidate.
#[derive(Clone)]
pub(super) struct Image {
    pub(super) width_px: u32,
    pub(super) height_px: u32,
    pub(super) pixels: Arc<[u8]>,
}

/// What a resource step would do, decided before anything changes.
pub(super) enum ResourcePlan {
    /// Begin admitted into `slot`.
    Admit(LockResourceBegin),
    /// Begin, End or a size check refused; the slot, if any, is freed.
    Reject {
        transaction: u64,
        resource: LockResourceId,
        reason: u16,
        slot: Option<usize>,
    },
    /// End delivered every byte: the upload in `slot` becomes an image.
    Accept {
        transaction: u64,
        resource: LockResourceId,
        slot: usize,
    },
    Cancel {
        transaction: u64,
        resource: LockResourceId,
        slot: usize,
    },
    Retire {
        transaction: u64,
        resource: LockResourceId,
    },
}

impl ResourcePlan {
    /// The status or release event this step journals.
    pub(super) fn event(&self) -> Result<(LockFileKind, Vec<u8>), Errno> {
        let status = |transaction, resource, status, reason, admitted_bytes| {
            LockResourceStatus {
                transaction,
                resource,
                status,
                reason,
                admitted_bytes,
            }
            .encode()
            .map(|body| (LockFileKind::ResourceStatus, body))
            .map_err(|_| Errno::EINVAL)
        };
        match *self {
            Self::Admit(begin) => status(
                begin.transaction,
                begin.resource,
                LockResourceState::Admitted,
                reason::NONE,
                begin.total_bytes(),
            ),
            Self::Reject {
                transaction,
                resource,
                reason,
                ..
            } => status(
                transaction,
                resource,
                LockResourceState::Rejected,
                reason,
                0,
            ),
            Self::Accept {
                transaction,
                resource,
                ..
            } => status(
                transaction,
                resource,
                LockResourceState::Accepted,
                reason::NONE,
                0,
            ),
            Self::Cancel {
                transaction,
                resource,
                ..
            } => status(
                transaction,
                resource,
                LockResourceState::Cancelled,
                reason::NONE,
                0,
            ),
            Self::Retire {
                transaction,
                resource,
            } => LockResourceReleased {
                transaction,
                resource,
                reason: reason::NONE,
            }
            .encode()
            .map(|body| (LockFileKind::ResourceReleased, body))
            .map_err(|_| Errno::EINVAL),
        }
    }
}

pub(super) struct Resources {
    limits: LockFileLimits,
    slots: [Option<Upload>; 4],
    images: BTreeMap<LockResourceId, Image>,
    next_binding: u64,
    /// Storage of retired, cancelled and rejected uploads, reused once
    /// nothing else holds it. A provider uploads whole frames continuously;
    /// fresh storage for each one costs a page fault per page. Live
    /// resources and spare storage together stay within the epoch's
    /// live-resource limit.
    spare: Vec<Arc<[u8]>>,
}

impl Resources {
    pub(super) fn new(limits: LockFileLimits) -> Self {
        Self {
            limits,
            slots: Default::default(),
            images: BTreeMap::new(),
            next_binding: 0,
            spare: Vec::new(),
        }
    }

    /// Unshared storage of exactly `len` bytes: spare storage nothing else
    /// holds any more, or new storage.
    fn storage(&mut self, len: usize) -> Arc<[u8]> {
        let reusable = self
            .spare
            .iter_mut()
            .position(|spare| spare.len() == len && Arc::get_mut(spare).is_some());
        match reusable {
            Some(index) => self.spare.swap_remove(index),
            None => std::iter::repeat_n(0, len).collect(),
        }
    }

    /// Keeps `pixels` for reuse, within the room the live resources leave.
    fn recycle(&mut self, pixels: Arc<[u8]>) {
        self.spare.push(pixels);
        self.trim_spare();
    }

    /// Drops spare storage beyond what the live resources leave of the
    /// epoch's limit, so storage held for reuse never adds to what an epoch
    /// may hold. Storage still shared goes first: it is the least likely to
    /// be reused.
    fn trim_spare(&mut self) {
        let room = usize::from(self.limits.max_live_resources).saturating_sub(self.live());
        while self.spare.len() > room {
            let index = self
                .spare
                .iter_mut()
                .position(|spare| Arc::get_mut(spare).is_none())
                .unwrap_or(0);
            self.spare.swap_remove(index);
        }
    }

    pub(super) fn image(&self, resource: LockResourceId) -> Option<&Image> {
        self.images.get(&resource)
    }

    fn uploading(&self, resource: LockResourceId) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|u| u.resource == resource))
    }

    fn live(&self) -> usize {
        self.images.len() + self.slots.iter().flatten().count()
    }

    /// Slot misuse and duplicate identities are the provider's protocol
    /// errors and fail the write; sizes and budget are refused by status.
    pub(super) fn plan_begin(&self, begin: LockResourceBegin) -> Result<ResourcePlan, Errno> {
        let slot = usize::from(begin.slot);
        if begin.slot >= self.limits.upload_slots || self.slots[slot].is_some() {
            return Err(Errno::EINVAL);
        }
        if self.images.contains_key(&begin.resource) || self.uploading(begin.resource).is_some() {
            return Err(Errno::EINVAL);
        }
        let reject = |reason| ResourcePlan::Reject {
            transaction: begin.transaction,
            resource: begin.resource,
            reason,
            slot: None,
        };
        if begin.width_px > self.limits.max_width_px
            || begin.height_px > self.limits.max_height_px
            || begin.total_bytes() > self.limits.max_resource_bytes
            || usize::try_from(begin.total_bytes()).is_err()
            || self.live() >= usize::from(self.limits.max_live_resources)
        {
            return Ok(reject(reason::BUDGET));
        }
        Ok(ResourcePlan::Admit(begin))
    }

    pub(super) fn plan_end(&self, step: LockResourceStep) -> Result<ResourcePlan, Errno> {
        let slot = self.slot_of(step)?;
        let upload = self.slots[slot].as_ref().ok_or(Errno::EINVAL)?;
        let whole = step.total_bytes == Some(upload.expected) && upload.written == upload.expected;
        Ok(if whole {
            ResourcePlan::Accept {
                transaction: step.transaction,
                resource: step.resource,
                slot,
            }
        } else {
            ResourcePlan::Reject {
                transaction: step.transaction,
                resource: step.resource,
                reason: reason::SIZE_MISMATCH,
                slot: Some(slot),
            }
        })
    }

    pub(super) fn plan_cancel(&self, step: LockResourceStep) -> Result<ResourcePlan, Errno> {
        Ok(ResourcePlan::Cancel {
            transaction: step.transaction,
            resource: step.resource,
            slot: self.slot_of(step)?,
        })
    }

    pub(super) fn plan_retire(&self, step: LockResourceStep) -> Result<ResourcePlan, Errno> {
        if !self.images.contains_key(&step.resource) {
            return Err(Errno::EINVAL);
        }
        Ok(ResourcePlan::Retire {
            transaction: step.transaction,
            resource: step.resource,
        })
    }

    fn slot_of(&self, step: LockResourceStep) -> Result<usize, Errno> {
        self.uploading(step.resource)
            .filter(|slot| {
                self.slots[*slot]
                    .as_ref()
                    .is_some_and(|u| u.transaction == step.transaction)
            })
            .ok_or(Errno::EINVAL)
    }

    /// Applies a journaled plan. Returns the image an accepted upload became.
    pub(super) fn apply(&mut self, plan: ResourcePlan) -> Option<(LockResourceId, Image)> {
        match plan {
            ResourcePlan::Admit(begin) => {
                self.next_binding += 1;
                // plan_begin admitted the size, so it fits in memory's index.
                let pixels = usize::try_from(begin.total_bytes())
                    .ok()
                    .map(|len| self.storage(len));
                self.slots[usize::from(begin.slot)] = Some(Upload {
                    binding: self.next_binding,
                    transaction: begin.transaction,
                    resource: begin.resource,
                    width_px: begin.width_px,
                    height_px: begin.height_px,
                    expected: begin.total_bytes(),
                    pixels,
                    written: 0,
                });
                self.trim_spare();
                None
            }
            ResourcePlan::Reject { slot, .. } => {
                if let Some(pixels) = slot.and_then(|slot| self.slots[slot].take()?.pixels) {
                    self.recycle(pixels);
                }
                None
            }
            ResourcePlan::Cancel { slot, .. } => {
                if let Some(pixels) = self.slots[slot].take().and_then(|upload| upload.pixels) {
                    self.recycle(pixels);
                }
                None
            }
            ResourcePlan::Accept { slot, resource, .. } => {
                let upload = self.slots[slot].take()?;
                let image = Image {
                    width_px: upload.width_px,
                    height_px: upload.height_px,
                    // Admit took the storage and every byte arrived (plan_end).
                    pixels: upload.pixels.unwrap_or_else(|| Arc::from([])),
                };
                self.images.insert(resource, image.clone());
                Some((resource, image))
            }
            ResourcePlan::Retire { resource, .. } => {
                // Session may still show it; the storage is reused only once
                // every other holder has let it go.
                if let Some(image) = self.images.remove(&resource) {
                    self.recycle(image.pixels);
                }
                None
            }
        }
    }

    /// Appends at the upload's exact cursor, within its declared size. A
    /// writer of an earlier binding of the slot is stale.
    pub(super) fn write(
        &mut self,
        slot: u8,
        binding: u64,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        let upload = self
            .slots
            .get_mut(usize::from(slot))
            .and_then(Option::as_mut)
            .filter(|upload| upload.binding == binding)
            .ok_or(Errno::ESTALE)?;
        let end = offset.checked_add(data.len() as u64).ok_or(Errno::EINVAL)?;
        if offset != upload.written || end > upload.expected {
            return Err(Errno::EINVAL);
        }
        if data.is_empty() {
            return Ok(0);
        }
        let storage = upload
            .pixels
            .as_mut()
            .and_then(Arc::get_mut)
            .ok_or(Errno::EINVAL)?;
        storage[offset as usize..end as usize].copy_from_slice(data);
        upload.written = end;
        Ok(data.len() as u32)
    }

    /// The binding `slot` holds, if any.
    pub(super) fn binding(&self, slot: u8) -> Option<u64> {
        self.slots
            .get(usize::from(slot))
            .and_then(Option::as_ref)
            .map(|upload| upload.binding)
    }
}
