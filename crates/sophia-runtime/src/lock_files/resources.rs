//! Immutable lock images and the leases that keep retired pixels charged.
//! Admission and validation belong to the image channel; this registry never
//! allocates writable pixel storage or binds byte-upload slots.
use std::collections::BTreeMap;

use sophia_protocol::lock_files::LockResourceId;

use super::leases::{IsbLease, IsbLeases};
use crate::SharedPixels;

#[derive(Clone)]
pub(super) struct Image {
    pub(super) width_px: u32,
    pub(super) height_px: u32,
    pub(super) pixels: SharedPixels,
}

pub(super) struct Resources {
    images: BTreeMap<LockResourceId, Image>,
    pub(super) leases: IsbLeases,
}

impl Resources {
    pub(super) fn new() -> Self {
        Self {
            images: BTreeMap::new(),
            leases: IsbLeases::default(),
        }
    }

    pub(super) fn image(&self, resource: LockResourceId) -> Option<&Image> {
        self.images.get(&resource)
    }

    pub(super) fn holds(&self, resource: LockResourceId) -> bool {
        self.images.contains_key(&resource) || self.leases.holds(resource)
    }

    /// Preserve every live or retiring lease across connection replacement.
    pub(super) fn take_isb_leases(&mut self) -> Vec<IsbLease> {
        let mut leases: Vec<_> = self
            .images
            .values()
            .map(|image| IsbLease::new(&image.pixels))
            .collect();
        leases.extend(self.leases.take());
        leases
    }

    pub(super) fn isb_bytes(&self) -> u64 {
        self.images
            .values()
            .map(|image| image.pixels.len() as u64)
            .sum::<u64>()
            + self.leases.bytes()
    }

    /// The offer has already reserved the live-resource and byte budgets.
    pub(super) fn insert_isb(
        &mut self,
        resource: LockResourceId,
        width_px: u32,
        height_px: u32,
        pixels: SharedPixels,
    ) {
        self.images.insert(
            resource,
            Image {
                width_px,
                height_px,
                pixels,
            },
        );
    }

    pub(super) fn live(&self) -> usize {
        self.images.len() + self.leases.len()
    }

    /// Called only after the retirement's receipt and release credit commit.
    pub(super) fn retire(&mut self, transaction: u64, resource: LockResourceId) {
        if let Some(Image { pixels, .. }) = self.images.remove(&resource) {
            self.leases.retire(transaction, resource, pixels);
        }
    }
}
