use crate::{OutputId, POLICY_MAX_SURFACES, Rect, SurfaceId};

pub const POLICY_MAX_OVERVIEW_WORKSPACES: usize = 16 * 63;
pub const POLICY_MAX_OVERVIEW_PLACEMENTS: usize = POLICY_MAX_SURFACES * 63;

/// A WM-authored view of a workspace, separate from the active scene layout.
/// Workspace identity is an opaque WM token. Engine alone samples the surface
/// images; these identifiers and rectangles never cross the shell boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyOverviewWorkspace {
    pub output: OutputId,
    pub workspace: u64,
    pub bounds: Rect,
    pub active: bool,
    pub focus: Option<SurfaceId>,
    pub placements: Vec<PolicyOverviewPlacement>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyOverviewPlacement {
    pub surface: SurfaceId,
    pub geometry: Rect,
}
