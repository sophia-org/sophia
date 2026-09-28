//! Passive descriptor-role records, extracted from Sophia 262eb82bc.
//! This typed model does not select a transport or grant admission.
use crate::OutputId;

mod validation;
pub use validation::*;

pub const SOPHIA_SHELL_REFERENCE_REVISION: u16 = 3;
pub const SOPHIA_SHELL_CAPABILITY_SHORTCUT_CATALOG: u64 = 1 << 3;
pub const SOPHIA_SHELL_CAPABILITY_REFERENCE_SHEET: u64 = 1 << 4;
pub const SOPHIA_SHELL_MAX_SHORTCUTS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellShortcut {
    pub slot: u16,
    pub chord: String,
    pub action: String,
    pub label: Option<String>,
    pub group: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellShortcutCatalog {
    pub connection_epoch: u64,
    pub generation: u64,
    pub entries: Vec<ShellShortcut>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ShellReferenceOperation {
    Startup = 0,
    Toggle = 1,
    Next = 2,
    Previous = 3,
    Dismiss = 4,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellReferenceRequest {
    pub connection_epoch: u64,
    pub catalog_generation: u64,
    pub request_generation: u64,
    pub output: OutputId,
    pub output_generation: u64,
    pub presentation_epoch: u64,
    pub operation: ShellReferenceOperation,
}
/// Bounded presentation intent. Colors are straight-alpha ARGB; lengths are
/// logical pixels. No screen geometry or renderer resources cross this wire.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellReferenceStyle {
    pub body_size: u16,
    pub title_size: u16,
    pub padding: u16,
    pub row_gap: u16,
    pub key_gap: u16,
    pub column_gap: u16,
    pub border: u16,
    pub margin: u16,
    pub columns: u16,
    pub colors: [u32; 6],
    pub title: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellReferenceEntry {
    pub slot: u16,
    pub key: String,
    pub label: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellReferenceCandidate {
    pub connection_epoch: u64,
    pub catalog_generation: u64,
    pub request_generation: u64,
    pub candidate_generation: u64,
    pub output: OutputId,
    pub visible: bool,
    pub page: u16,
    pub style: ShellReferenceStyle,
    pub entries: Vec<ShellReferenceEntry>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellReferenceOutcome {
    pub connection_epoch: u64,
    pub catalog_generation: u64,
    pub request_generation: u64,
    pub candidate_generation: u64,
    pub presentation_epoch: u64,
    pub page: u16,
    pub pages: u16,
    pub kind: crate::ShellV1CandidateOutcomeKind,
}
