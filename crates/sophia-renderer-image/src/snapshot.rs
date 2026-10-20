use crate::RendererImageId;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

#[derive(Clone, Copy, Debug)]
pub struct DmaBufPlane<'a> {
    pub fd: BorrowedFd<'a>,
    pub offset: u32,
    pub stride: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct MultiPlaneDmaBufFrame<'a> {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub modifier: u64,
    pub plane_count: u8,
    pub planes: [Option<DmaBufPlane<'a>>; 4],
}

#[derive(Debug)]
pub struct RendererImageSnapshot {
    // Duplicated plane FDs keep pixels alive without retaining the old DRM
    // device, EGL display, or GBM context across a renderer generation change.
    pub(crate) image_id: RendererImageId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: u32,
    pub(crate) modifier: u64,
    pub(crate) plane_count: u8,
    pub(crate) planes: [Option<OwnedDmaBufPlane>; 4],
    pub(crate) charge: Option<std::sync::Arc<crate::custody::SnapshotCharge>>,
    pub(crate) epoch: Option<std::sync::Arc<crate::SnapshotEpoch>>,
}

#[derive(Debug)]
pub struct OwnedDmaBufPlane {
    pub fd: OwnedFd,
    pub offset: u32,
    pub stride: u32,
}

impl RendererImageSnapshot {
    /// Duplicate only descriptor ownership, preserving the original snapshot
    /// through a fallible replacement-renderer import.
    pub fn try_clone(&self) -> std::io::Result<Self> {
        let mut planes = std::array::from_fn(|_| None);
        for (destination, source) in planes.iter_mut().zip(&self.planes) {
            if let Some(source) = source {
                *destination = Some(OwnedDmaBufPlane {
                    fd: source.fd.try_clone()?,
                    offset: source.offset,
                    stride: source.stride,
                });
            }
        }
        Ok(Self {
            image_id: self.image_id,
            width: self.width,
            height: self.height,
            format: self.format,
            modifier: self.modifier,
            plane_count: self.plane_count,
            planes,
            charge: self.charge.clone(),
            epoch: self.epoch.clone(),
        })
    }

    pub const fn image_id(&self) -> RendererImageId {
        self.image_id
    }

    pub fn is_current(&self) -> bool {
        self.epoch.as_ref().is_none_or(|epoch| epoch.is_valid())
    }

    pub fn as_frame(&self) -> MultiPlaneDmaBufFrame<'_> {
        MultiPlaneDmaBufFrame {
            width: self.width,
            height: self.height,
            format: self.format,
            modifier: self.modifier,
            plane_count: self.plane_count,
            planes: std::array::from_fn(|index| {
                self.planes[index].as_ref().map(|plane| DmaBufPlane {
                    fd: plane.fd.as_fd(),
                    offset: plane.offset,
                    stride: plane.stride,
                })
            }),
        }
    }
}

impl MultiPlaneDmaBufFrame<'_> {
    pub fn is_valid(&self) -> bool {
        const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;
        const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241;
        self.width > 0
            && self.height > 0
            && matches!(self.format, DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888)
            && self.plane_count > 0
            && usize::from(self.plane_count) <= self.planes.len()
            && self.planes[..usize::from(self.plane_count)]
                .iter()
                .all(Option::is_some)
    }
}

/// Uncharged descriptor ownership, not an immutable-capture or completion proof.
/// Only the renderer owner may use this as the input to its retention path.
#[derive(Debug)]
pub struct OwnedSnapshotData {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub modifier: u64,
    pub plane_count: u8,
    pub planes: [Option<OwnedDmaBufPlane>; 4],
}

impl RendererImageSnapshot {
    /// Own exported descriptors without asserting successful capture, retention,
    /// or ended GPU reads. Charging storage does not establish those facts either.
    pub fn uncharged(image_id: RendererImageId, data: OwnedSnapshotData) -> Self {
        Self {
            image_id,
            width: data.width,
            height: data.height,
            format: data.format,
            modifier: data.modifier,
            plane_count: data.plane_count,
            planes: data.planes,
            charge: None,
            epoch: None,
        }
    }

    /// Whether this storage carries the budget charge and renderer epoch.
    /// This is an ownership fact, never a source-read completion proof.
    pub fn is_retained(&self) -> bool {
        self.charge.is_some() && self.epoch.is_some()
    }

    /// Whether this retained snapshot is the only tracked storage owner now.
    ///
    /// Same-object consumers hold this Arc; descriptor duplicates made by
    /// `try_clone` hold a different snapshot but share its budget charge. Both
    /// must drain before an owner may retire its resource accounting. This
    /// observation is not GPU completion, and does not evict cached imports.
    /// Callers must first stop issuing new leases and arrange native eviction;
    /// waiting for uniqueness alone cannot retire an idle import cache.
    pub fn custody_is_exclusive(self: &std::sync::Arc<Self>) -> bool {
        std::sync::Arc::strong_count(self) == 1
            && self.epoch.is_some()
            && self
                .charge
                .as_ref()
                .is_some_and(|charge| std::sync::Arc::strong_count(charge) == 1)
    }
}

#[cfg(test)]
#[path = "../tests/support/image_snapshot_ownership.rs"]
mod ownership_tests;
