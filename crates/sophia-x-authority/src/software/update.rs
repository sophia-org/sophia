use std::collections::BTreeMap;
use std::sync::Arc;

use sophia_protocol::{Rect, Size};

use crate::XResourceId;

use super::raster_ops::clipped_bounds;

mod tests;

pub const X_AUTHORITY_CPU_PATCH_BATCH_MAX_RECTS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XAuthorityCpuBufferSnapshot {
    pub handle: u64,
    pub drawable: XResourceId,
    pub size: Size,
    pub stride: u32,
    pub format: u32,
    pub generation: u64,
    /// Shared, and copied only when somebody else still reads the old bytes.
    ///
    /// Publishing a snapshot used to mean copying it: the authority cloned into
    /// the transport, the session cloned into the registry, the scene cloned
    /// per density variant, and the backend cloned again per head. Sharing the
    /// allocation makes each of those a refcount bump, and `Arc::make_mut` at
    /// the mutation sites keeps the guarantee that paid for the copies -- a
    /// presentation handed these bytes keeps reading them until it retires.
    ///
    /// Equality still compares contents. Two allocations may hold identical
    /// pixels, and a published snapshot is compared for what it says rather
    /// than for where it lives.
    pub bytes: Arc<Vec<u8>>,
}

/// What a core drawing operation did, without the pixels it did it to.
///
/// Core drawing rebrands the drawable's snapshot with a fresh handle and then
/// composes it into the toplevel's presentation buffer, which is the update the
/// session actually publishes. Every caller of a drawing operation reads only
/// the handle from this half, so returning the snapshot meant cloning a whole
/// buffer for a caller that indexed one field out of it -- for a 1080p toplevel,
/// eight megabytes copied and dropped per draw.
///
/// Passive by construction: it names what happened and owns nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityCpuDrawResult {
    pub handle: u64,
    pub size: Size,
    pub generation: u64,
}

impl XAuthorityCpuDrawResult {
    #[must_use]
    pub const fn handle(&self) -> u64 {
        self.handle
    }

    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XAuthorityCpuBufferPatch {
    pub handle: u64,
    pub drawable: XResourceId,
    pub size: Size,
    pub stride: u32,
    pub format: u32,
    pub generation: u64,
    pub rect: Rect,
    /// Owned packed pixels, shared across forwarding and admission queues.
    /// They never alias the drawable that subsequent requests can modify.
    pub bytes: Arc<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XAuthorityCpuBufferPatchRegion {
    pub rect: Rect,
    pub bytes: Arc<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XAuthorityCpuBufferPatchBatch {
    pub handle: u64,
    pub drawable: XResourceId,
    pub size: Size,
    pub stride: u32,
    pub format: u32,
    pub generation: u64,
    pub patches: Vec<XAuthorityCpuBufferPatchRegion>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XAuthorityCpuBufferUpdate {
    Replace(XAuthorityCpuBufferSnapshot),
    Patch(XAuthorityCpuBufferPatch),
    PatchBatch(XAuthorityCpuBufferPatchBatch),
}

impl XAuthorityCpuBufferUpdate {
    pub const fn handle(&self) -> u64 {
        match self {
            Self::Replace(snapshot) => snapshot.handle,
            Self::Patch(patch) => patch.handle,
            Self::PatchBatch(batch) => batch.handle,
        }
    }

    /// The pixel format the update's bytes are written in.
    ///
    /// Carried by every variant because a shaped presentation ships alpha
    /// and an unshaped one does not, and the receiver has to read the bytes
    /// under the format they were written in.
    pub const fn format(&self) -> u32 {
        match self {
            Self::Replace(snapshot) => snapshot.format,
            Self::Patch(patch) => patch.format,
            Self::PatchBatch(batch) => batch.format,
        }
    }

    pub const fn generation(&self) -> u64 {
        match self {
            Self::Replace(snapshot) => snapshot.generation,
            Self::Patch(patch) => patch.generation,
            Self::PatchBatch(batch) => batch.generation,
        }
    }

    pub const fn size(&self) -> Size {
        match self {
            Self::Replace(snapshot) => snapshot.size,
            Self::Patch(patch) => patch.size,
            Self::PatchBatch(batch) => batch.size,
        }
    }

    pub fn payload_bytes(&self) -> usize {
        match self {
            Self::Replace(snapshot) => snapshot.bytes.len(),
            Self::Patch(patch) => patch.bytes.len(),
            Self::PatchBatch(batch) => batch.patches.iter().fold(0usize, |total, patch| {
                total.saturating_add(patch.bytes.len())
            }),
        }
    }

    pub fn patch_rects(&self) -> usize {
        match self {
            Self::Replace(_) => 0,
            Self::Patch(_) => 1,
            Self::PatchBatch(batch) => batch.patches.len(),
        }
    }

    pub const fn is_replacement(&self) -> bool {
        matches!(self, Self::Replace(_))
    }

    pub fn apply_to(
        &self,
        buffers: &mut BTreeMap<u64, XAuthorityCpuBufferSnapshot>,
    ) -> Result<(), &'static str> {
        match self {
            Self::Replace(snapshot) => {
                buffers.insert(snapshot.handle, snapshot.clone());
                Ok(())
            }
            Self::Patch(patch) => {
                let buffer = buffers
                    .get_mut(&patch.handle)
                    .ok_or("CPU buffer patch has no replacement base")?;
                if buffer.drawable != patch.drawable
                    || buffer.size != patch.size
                    || buffer.stride != patch.stride
                    || buffer.format != patch.format
                    || patch.generation < buffer.generation
                {
                    return Err("CPU buffer patch metadata does not match its base");
                }
                apply_packed_patch(buffer, patch)?;
                buffer.generation = patch.generation;
                Ok(())
            }
            Self::PatchBatch(batch) => {
                let buffer = buffers
                    .get_mut(&batch.handle)
                    .ok_or("CPU buffer patch batch has no replacement base")?;
                if buffer.drawable != batch.drawable
                    || buffer.size != batch.size
                    || buffer.stride != batch.stride
                    || buffer.format != batch.format
                    || batch.generation < buffer.generation
                    || batch.patches.len() > X_AUTHORITY_CPU_PATCH_BATCH_MAX_RECTS
                {
                    return Err("CPU buffer patch batch metadata does not match its base");
                }
                for patch in &batch.patches {
                    validate_packed_patch_region(buffer, patch)?;
                }
                for patch in &batch.patches {
                    apply_packed_patch_region(buffer, patch)?;
                }
                buffer.generation = batch.generation;
                Ok(())
            }
        }
    }
}

/// Whether a damage rectangle names bytes this buffer actually holds.
///
/// Every refusal `packed_patch` can make about a rectangle is made here, so a
/// caller that only needs the answer does not have to copy the bytes to get it.
/// Splitting them apart is what lets a drawing operation validate its damage
/// without building an update nobody reads; keeping one implementation is what
/// stops the two answers drifting.
pub(super) fn patch_is_representable(
    buffer: &XAuthorityCpuBufferSnapshot,
    rect: Rect,
) -> Option<()> {
    let (left, top, right, bottom) = clipped_bounds(buffer.size, rect)?;
    let width = right.saturating_sub(left);
    let row_bytes = width.checked_mul(4)?;
    let source_stride = usize::try_from(buffer.stride).ok()?;
    row_bytes.checked_mul(bottom.saturating_sub(top))?;
    for y in top..bottom {
        let offset = y
            .checked_mul(source_stride)?
            .checked_add(left.checked_mul(4)?)?;
        buffer
            .bytes
            .get(offset..offset.checked_add(row_bytes)?)
            .map(|_| ())?;
    }
    i32::try_from(left).ok()?;
    i32::try_from(top).ok()?;
    i32::try_from(width).ok()?;
    i32::try_from(bottom.saturating_sub(top)).ok()?;
    Some(())
}

pub(super) fn packed_patch(
    buffer: &XAuthorityCpuBufferSnapshot,
    rect: Rect,
) -> Option<XAuthorityCpuBufferPatch> {
    patch_is_representable(buffer, rect)?;
    let (left, top, right, bottom) = clipped_bounds(buffer.size, rect)?;
    let width = right.saturating_sub(left);
    let height = bottom.saturating_sub(top);
    let row_bytes = width.checked_mul(4)?;
    let source_stride = usize::try_from(buffer.stride).ok()?;
    let mut bytes = Vec::with_capacity(row_bytes.checked_mul(height)?);
    for y in top..bottom {
        let offset = y
            .checked_mul(source_stride)?
            .checked_add(left.checked_mul(4)?)?;
        bytes.extend_from_slice(buffer.bytes.get(offset..offset.checked_add(row_bytes)?)?);
    }
    Some(XAuthorityCpuBufferPatch {
        handle: buffer.handle,
        drawable: buffer.drawable,
        size: buffer.size,
        stride: buffer.stride,
        format: buffer.format,
        generation: buffer.generation,
        rect: Rect {
            x: i32::try_from(left).ok()?,
            y: i32::try_from(top).ok()?,
            width: i32::try_from(width).ok()?,
            height: i32::try_from(height).ok()?,
        },
        bytes: Arc::new(bytes),
    })
}

pub(super) fn packed_patch_region(
    buffer: &XAuthorityCpuBufferSnapshot,
    rect: Rect,
) -> Option<XAuthorityCpuBufferPatchRegion> {
    let patch = packed_patch(buffer, rect)?;
    Some(XAuthorityCpuBufferPatchRegion {
        rect: patch.rect,
        bytes: patch.bytes,
    })
}

fn apply_packed_patch(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    patch: &XAuthorityCpuBufferPatch,
) -> Result<(), &'static str> {
    let (left, top, right, bottom) =
        clipped_bounds(buffer.size, patch.rect).ok_or("CPU buffer patch is empty")?;
    if patch.rect.x != i32::try_from(left).unwrap_or(i32::MAX)
        || patch.rect.y != i32::try_from(top).unwrap_or(i32::MAX)
        || patch.rect.width != i32::try_from(right.saturating_sub(left)).unwrap_or(i32::MAX)
        || patch.rect.height != i32::try_from(bottom.saturating_sub(top)).unwrap_or(i32::MAX)
    {
        return Err("CPU buffer patch lies outside its buffer");
    }
    let row_bytes = right.saturating_sub(left).saturating_mul(4);
    let expected = row_bytes.saturating_mul(bottom.saturating_sub(top));
    if patch.bytes.len() != expected {
        return Err("CPU buffer patch byte length is invalid");
    }
    let target_stride = usize::try_from(buffer.stride).map_err(|_| "invalid target stride")?;
    // Checked once per patch rather than once per row: a patch is a single
    // logical write, and splitting it halfway would leave two allocations
    // each holding part of the result.
    let target_bytes = Arc::make_mut(&mut buffer.bytes);
    for (row, y) in (top..bottom).enumerate() {
        let source_offset = row.saturating_mul(row_bytes);
        let target_offset = y
            .saturating_mul(target_stride)
            .saturating_add(left.saturating_mul(4));
        let source = patch
            .bytes
            .get(source_offset..source_offset.saturating_add(row_bytes))
            .ok_or("CPU buffer patch source row is invalid")?;
        let target = target_bytes
            .get_mut(target_offset..target_offset.saturating_add(row_bytes))
            .ok_or("CPU buffer patch target row is invalid")?;
        target.copy_from_slice(source);
    }
    Ok(())
}

fn validate_packed_patch_region(
    buffer: &XAuthorityCpuBufferSnapshot,
    patch: &XAuthorityCpuBufferPatchRegion,
) -> Result<(), &'static str> {
    let (left, top, right, bottom) =
        clipped_bounds(buffer.size, patch.rect).ok_or("CPU buffer patch is empty")?;
    if patch.rect.x != i32::try_from(left).unwrap_or(i32::MAX)
        || patch.rect.y != i32::try_from(top).unwrap_or(i32::MAX)
        || patch.rect.width != i32::try_from(right.saturating_sub(left)).unwrap_or(i32::MAX)
        || patch.rect.height != i32::try_from(bottom.saturating_sub(top)).unwrap_or(i32::MAX)
    {
        return Err("CPU buffer patch lies outside its buffer");
    }
    let expected = right
        .saturating_sub(left)
        .saturating_mul(4)
        .saturating_mul(bottom.saturating_sub(top));
    (patch.bytes.len() == expected)
        .then_some(())
        .ok_or("CPU buffer patch byte length is invalid")
}

fn apply_packed_patch_region(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    patch: &XAuthorityCpuBufferPatchRegion,
) -> Result<(), &'static str> {
    validate_packed_patch_region(buffer, patch)?;
    let left = usize::try_from(patch.rect.x).map_err(|_| "CPU buffer patch x is invalid")?;
    let top = usize::try_from(patch.rect.y).map_err(|_| "CPU buffer patch y is invalid")?;
    let width =
        usize::try_from(patch.rect.width).map_err(|_| "CPU buffer patch width is invalid")?;
    let height =
        usize::try_from(patch.rect.height).map_err(|_| "CPU buffer patch height is invalid")?;
    let row_bytes = width.saturating_mul(4);
    let target_stride = usize::try_from(buffer.stride).map_err(|_| "invalid target stride")?;
    // Checked once per patch rather than once per row: a patch is a single
    // logical write, and splitting it halfway would leave two allocations
    // each holding part of the result.
    let target_bytes = Arc::make_mut(&mut buffer.bytes);
    for row in 0..height {
        let source_offset = row.saturating_mul(row_bytes);
        let target_offset = top
            .saturating_add(row)
            .saturating_mul(target_stride)
            .saturating_add(left.saturating_mul(4));
        let source = patch
            .bytes
            .get(source_offset..source_offset.saturating_add(row_bytes))
            .ok_or("CPU buffer patch source row is invalid")?;
        let target = target_bytes
            .get_mut(target_offset..target_offset.saturating_add(row_bytes))
            .ok_or("CPU buffer patch target row is invalid")?;
        target.copy_from_slice(source);
    }
    Ok(())
}
