//! Passive, deliberately lossy host inspection records. These are not WM
//! messages: no profile, operation, launch, or action authority is carried.
//! All u64 values use canonical decimal JSON strings, including capabilities.

mod codec;
mod format;

pub use codec::*;
pub use format::*;

use serde::{Deserialize, Serialize};

pub const INSPECTION_SCHEMA: u32 = 1;
pub const INSPECTION_MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
pub const INSPECTION_MAX_RING_BYTES: usize = 1024 * 1024;
pub const INSPECTION_MAX_EVENTS: usize = 64;
pub const INSPECTION_MAX_OUTPUTS: usize = 16;
pub const INSPECTION_MAX_SURFACES: usize = 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionWire {
    CurrentIpc,
    Files,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionState {
    Starting,
    Ready,
    Unavailable,
    Stopped,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectionRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectionSurfaceId {
    pub index: u32,
    pub generation: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectionOutput {
    #[serde(with = "codec::decimal")]
    pub id: u64,
    #[serde(with = "codec::decimal")]
    pub generation: u64,
    pub geometry: InspectionRect,
    pub work_area: InspectionRect,
    pub focus: Option<InspectionSurfaceId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectionSurface {
    pub id: InspectionSurfaceId,
    #[serde(with = "codec::decimal")]
    pub state_generation: u64,
    #[serde(with = "codec::optional_decimal")]
    pub output: Option<u64>,
    pub geometry: InspectionRect,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectionSnapshot {
    #[serde(with = "codec::decimal")]
    pub session_generation: u64,
    #[serde(with = "codec::decimal")]
    pub wm_epoch: u64,
    #[serde(with = "codec::decimal")]
    pub scene_generation: u64,
    #[serde(with = "codec::decimal")]
    pub selected_capabilities: u64,
    pub wire: InspectionWire,
    pub state: InspectionState,
    #[serde(deserialize_with = "codec::outputs")]
    pub outputs: Vec<InspectionOutput>,
    #[serde(deserialize_with = "codec::surfaces")]
    pub surfaces: Vec<InspectionSurface>,
}

/// Owner reports only. None of these names certifies physical completion.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionEvent {
    SnapshotChanged,
    ConnectionChanged,
    ConfigurationChanged,
    ConfigurationRejected,
    ProjectionCommitted,
    ProjectionRejected,
    ProjectionTimedOut,
    PresentationChanged,
    SessionOperationAccepted,
    SessionOperationRejected,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectionSnapshotRecord {
    pub schema: u32,
    /// Private observer fence, independent of WM connection epochs.
    #[serde(with = "codec::decimal")]
    pub generation: u64,
    #[serde(with = "codec::decimal")]
    pub sequence: u64,
    /// First event byte after this atomic snapshot publication.
    #[serde(with = "codec::decimal")]
    pub event_offset: u64,
    #[serde(with = "codec::decimal")]
    pub loss_generation: u64,
    pub snapshot: InspectionSnapshot,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectionEventRecord {
    pub schema: u32,
    #[serde(with = "codec::decimal")]
    pub generation: u64,
    #[serde(with = "codec::decimal")]
    pub sequence: u64,
    #[serde(with = "codec::decimal")]
    pub loss_generation: u64,
    pub event: InspectionEvent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectionStatus {
    pub schema: u32,
    #[serde(with = "codec::decimal")]
    pub session_generation: u64,
    pub state: InspectionState,
    pub wire: Option<InspectionWire>,
    #[serde(with = "codec::decimal")]
    pub selected_capabilities: u64,
    #[serde(with = "codec::decimal")]
    pub generation: u64,
    #[serde(with = "codec::decimal")]
    pub sequence: u64,
    #[serde(with = "codec::decimal")]
    pub wm_epoch: u64,
    #[serde(with = "codec::decimal")]
    pub event_floor: u64,
    #[serde(with = "codec::decimal")]
    pub event_tail: u64,
    #[serde(with = "codec::decimal")]
    pub loss_generation: u64,
    pub snapshot_available: bool,
}
