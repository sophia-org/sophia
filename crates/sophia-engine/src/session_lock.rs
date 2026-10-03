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
    CompositorDisplayCommand, CompositorDisplayList, CompositorNodeId, CompositorRect,
    CompositorRgb8, OutputFrameDamageSnapshot,
};

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

/// What Engine draws on every head while the session is locked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLockCover {
    pub epoch: SessionLockEpoch,
    pub fill: CompositorRgb8,
}

impl SessionLockCover {
    /// The display list of a locked output: the opaque fill over the whole
    /// logical viewport, and nothing else. Letterbox bars outside the
    /// viewport are the planner's black background.
    pub fn display_list<C>(&self, output: OutputId, viewport: Rect) -> CompositorDisplayList<C> {
        CompositorDisplayList {
            output,
            commands: vec![CompositorDisplayCommand::Rect(CompositorRect {
                opacity: u8::MAX,
                node: CompositorNodeId::SessionLock {
                    output,
                    epoch: self.epoch.raw(),
                },
                generation: self.epoch.raw(),
                geometry: viewport,
                color: self.fill,
            })],
        }
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
    for command in &frame.compositor_display_list.commands {
        let CompositorDisplayCommand::Rect(rect) = command else {
            return None;
        };
        let CompositorNodeId::SessionLock {
            output: node_output,
            epoch: raw,
        } = rect.node
        else {
            return None;
        };
        if node_output != output || rect.opacity != u8::MAX || rect.geometry.is_empty() {
            return None;
        }
        let drawn = SessionLockEpoch::from_raw(raw)?;
        if *epoch.get_or_insert(drawn) != drawn {
            return None;
        }
    }
    epoch
}
