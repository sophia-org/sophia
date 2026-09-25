use crate::OutputId;

pub const SOPHIA_SHELL_OVERVIEW_REVISION: u16 = 9;
pub const SOPHIA_SHELL_CAPABILITY_OVERVIEW: u64 = 1 << 13;
pub const SOPHIA_SHELL_MAX_OVERVIEW_WORKSPACES: usize = 1008;
pub const SOPHIA_SHELL_MAX_OVERVIEW_WINDOWS: usize = 64512;
pub const SOPHIA_SHELL_MAX_OVERVIEW_WINDOWS_PER_WORKSPACE: usize = 1024;

/// Catalog-local slots only. The session retains all slot-to-scene mappings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellOverviewWorkspace {
    pub slot: u16,
    pub output: OutputId,
    pub active: bool,
    pub focused: u16,
    pub windows: Vec<u16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellOverviewCatalog {
    pub connection_epoch: u64,
    pub generation: u64,
    pub workspaces: Vec<ShellOverviewWorkspace>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ShellOverviewOperation {
    Toggle = 1,
    Dismiss = 2,
    Left = 3,
    Right = 4,
    Up = 5,
    Down = 6,
    First = 7,
    Last = 8,
    Accept = 9,
    Pick = 10,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellOverviewRequest {
    pub connection_epoch: u64,
    pub catalog_generation: u64,
    pub request_generation: u64,
    pub presentation_epoch: u64,
    pub output: OutputId,
    pub operation: ShellOverviewOperation,
    pub workspace: u16,
    pub window: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellOverviewCandidate {
    pub connection_epoch: u64,
    pub catalog_generation: u64,
    pub request_generation: u64,
    pub candidate_generation: u64,
    pub visible: bool,
    pub activate: bool,
    pub workspace: u16,
    pub window: u16,
}
