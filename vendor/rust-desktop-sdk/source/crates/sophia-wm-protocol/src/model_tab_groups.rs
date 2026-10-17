use crate::{OutputId, Rect, SurfaceId};

pub const POLICY_MAX_TAB_GROUPS: usize = 1024;
pub const POLICY_MAX_TAB_MEMBERS: usize = 2048;
pub const POLICY_TAB_HEIGHT: i32 = 24;

/// Flat presentation intent. A group is not a policy tree node; only policy
/// knows how selecting a member changes its private layout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyTabGroup {
    pub output: OutputId,
    pub group: u64,
    pub geometry: Rect,
    pub focused: bool,
    pub selected: Option<SurfaceId>,
    pub members: Vec<SurfaceId>,
}
