//! The session lock cover: what every head shows while the session is locked.
//!
//! Session owns the lock state and decides when it begins and ends; Engine
//! owns what a locked head draws. A locked output's display list is replaced
//! whole, not overlaid: it holds an opaque Engine fill over the output and,
//! later, the lock provider's admitted images, and nothing else. No
//! application surface, WM presentation, shell content or descriptor chrome is
//! named in it, so none is sampled, and a renderer fault cannot show one
//! through the fill.
//!
//! The fill is also the proof. It is an ordinary compositor rect whose node
//! carries the lock epoch, so a retired head frame shows which lock it drew,
//! and [`presented_session_lock`] reads that back from the frames every head
//! last retired. A side marker could claim coverage the draw did not have; the
//! drawn rect cannot.

use crate::prelude::*;
use crate::{
    CompositorContentImage, CompositorDisplayCommand, CompositorDisplayList, CompositorImageSource,
    CompositorImageSourceIdentity, CompositorNodeId, CompositorRect, CompositorRgb8,
    OutputFrameDamageSnapshot,
};
use std::collections::BTreeMap;
use std::sync::Arc;

/// One lock, from the moment Session begins it until it ends. Epochs are
/// minted by Session and never reused, so a frame drawn for one lock never
/// proves another.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SessionLockEpoch(core::num::NonZeroU64);

impl SessionLockEpoch {
    pub const FIRST: Self = Self(core::num::NonZeroU64::MIN);

    /// Zero is never an epoch.
    pub const fn from_raw(raw: u64) -> Option<Self> {
        match core::num::NonZeroU64::new(raw) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }

    pub const fn raw(self) -> u64 {
        self.0.get()
    }

    /// The following epoch; `None` at exhaustion, which refuses the lock
    /// rather than reusing an epoch.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// Which provider image a lock shows: its connection epoch, so a replaced
/// provider's image is never the same identity as its successor's, and its
/// resource. Comparable and pixel-free.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SessionLockImageIdentity {
    pub output: OutputId,
    pub connection_epoch: u64,
    pub resource_id: u64,
    pub resource_generation: u64,
}

/// A lock provider's whole image for one output, premultiplied BGRA8, sized
/// exactly to that output's allocation.
#[derive(Clone)]
pub struct SessionLockImage {
    pub identity: SessionLockImageIdentity,
    pub width_px: u32,
    pub height_px: u32,
    pub pixels: std::sync::Arc<[u8]>,
}

impl PartialEq for SessionLockImage {
    /// The identity names the pixels: a resource is immutable once whole.
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.width_px == other.width_px
            && self.height_px == other.height_px
    }
}

impl Eq for SessionLockImage {}

/// The provider image an output shows over the fill, and the candidate
/// generation that placed it there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionLockImagePlacement {
    pub image: SessionLockImage,
    pub generation: u64,
}

impl core::fmt::Debug for SessionLockImage {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("SessionLockImage")
            .field("identity", &self.identity)
            .field("width_px", &self.width_px)
            .field("height_px", &self.height_px)
            .finish_non_exhaustive()
    }
}

/// What Engine draws on every head while the session is locked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionLockCover {
    pub epoch: SessionLockEpoch,
    pub fill: CompositorRgb8,
    /// The provider image each output shows over the fill, if any. Shared,
    /// so the cover stays cheap to hand to every frame.
    pub images: Arc<BTreeMap<OutputId, SessionLockImagePlacement>>,
}

impl SessionLockCover {
    /// A cover of the fill alone.
    pub fn fill(epoch: SessionLockEpoch, fill: CompositorRgb8) -> Self {
        Self {
            epoch,
            fill,
            images: Arc::default(),
        }
    }

    /// The display list of a locked output: the opaque fill over the whole
    /// logical viewport and, above it, the provider's image for this output
    /// if it has one, and nothing else. The image is placed in its own pixel
    /// space, which the head planner scales to every head of the output.
    /// Letterbox bars outside the viewport are the planner's black
    /// background.
    pub fn display_list(&self, output: OutputId, viewport: Rect) -> CompositorDisplayList {
        let node = CompositorNodeId::SessionLock {
            output,
            epoch: self.epoch.raw(),
        };
        let mut commands = vec![CompositorDisplayCommand::Rect(CompositorRect {
            opacity: u8::MAX,
            node,
            generation: self.epoch.raw(),
            geometry: viewport,
            color: self.fill,
        })];
        if let Some(placement) = self.images.get(&output)
            && placement.image.identity.output == output
        {
            let node = CompositorNodeId::SessionLockImage {
                output,
                epoch: self.epoch.raw(),
            };
            let image = &placement.image;
            let size = Size {
                width: i32::try_from(image.width_px).unwrap_or(i32::MAX),
                height: i32::try_from(image.height_px).unwrap_or(i32::MAX),
            };
            commands.push(CompositorDisplayCommand::ContentImage(
                CompositorContentImage {
                    node,
                    generation: placement.generation,
                    output_size_px: size,
                    geometry_px: Rect {
                        x: 0,
                        y: 0,
                        width: size.width,
                        height: size.height,
                    },
                    size_px: size,
                    stride: image.width_px.saturating_mul(4),
                    format: u32::from_le_bytes(*b"AR24"),
                    resource: CompositorImageSource::Lock(image.clone()),
                },
            ));
        }
        CompositorDisplayList { output, commands }
    }
}

/// The lock a whole output presents, read from the frame each of its heads
/// last retired (primary first, as the native target reports them).
///
/// `Some` only when every head retired a frame drawn for the same lock: no
/// surface, no surface instance and no compositor content except that lock's
/// own fill on that output. A head that has retired nothing yet, still shows an
/// earlier frame, or drew anything else leaves the output unproven, which the
/// caller must treat as not locked.
pub fn presented_session_lock(
    output: OutputId,
    frames: &[Option<&OutputFrameDamageSnapshot>],
) -> Option<SessionLockEpoch> {
    let mut proven = None;
    if frames.is_empty() {
        return None;
    }
    for frame in frames {
        let epoch = frame_session_lock(output, (*frame)?)?;
        if *proven.get_or_insert(epoch) != epoch {
            return None;
        }
    }
    proven
}

fn frame_session_lock(
    output: OutputId,
    frame: &OutputFrameDamageSnapshot,
) -> Option<SessionLockEpoch> {
    if !frame.surfaces.is_empty() || frame.compositor_display_list.output != output {
        return None;
    }
    let mut epoch = None;
    let mut filled = false;
    for command in &frame.compositor_display_list.commands {
        // The fill, and the provider's image above it, on this output's
        // lock node; anything else voids the proof.
        let (node_output, raw) = match command {
            CompositorDisplayCommand::Rect(rect) => {
                if rect.opacity != u8::MAX || rect.geometry.is_empty() {
                    return None;
                }
                let CompositorNodeId::SessionLock { output, epoch } = rect.node else {
                    return None;
                };
                filled = true;
                (output, epoch)
            }
            CompositorDisplayCommand::ContentImage(image)
                if matches!(image.resource, CompositorImageSourceIdentity::Lock(_)) =>
            {
                let CompositorNodeId::SessionLockImage { output, epoch } = image.node else {
                    return None;
                };
                (output, epoch)
            }
            _ => return None,
        };
        if node_output != output {
            return None;
        }
        let drawn = SessionLockEpoch::from_raw(raw)?;
        if *epoch.get_or_insert(drawn) != drawn {
            return None;
        }
    }
    epoch.filter(|_| filled)
}

/// The provider image a whole output presents, and its candidate
/// generation: every head of the output must have retired a locked frame
/// showing the same one.
pub fn presented_session_lock_image(
    output: OutputId,
    frames: &[Option<&OutputFrameDamageSnapshot>],
) -> Option<(SessionLockImageIdentity, u64)> {
    presented_session_lock(output, frames)?;
    let mut shown = None;
    for frame in frames {
        let image =
            (*frame)?.compositor_display_list.commands.iter().find_map(
                |command| match command {
                    CompositorDisplayCommand::ContentImage(image) => match &image.resource {
                        CompositorImageSourceIdentity::Lock(identity) => {
                            Some((*identity, image.generation))
                        }
                        CompositorImageSourceIdentity::Shell(_) => None,
                    },
                    _ => None,
                },
            )?;
        if *shown.get_or_insert(image) != image {
            return None;
        }
    }
    shown
}
