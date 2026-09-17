use crate::{
    ContentAllocationId, ContentCandidateBegin, ContentCandidateChunk, ContentGrant,
    ContentMargins, ContentOutputId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeLauncherOpening {
    pub grant: ContentGrant,
    pub opening: u64,
    pub output: ContentOutputId,
    pub catalog_generation: u64,
    pub state_revision: u64,
}

/// A parentless transient allocation, never a panel reservation or popout.
/// The reply is the existing ContentAllocationResult with a zero parent/anchor
/// and zero allowed reservation. Operation: acquire=1, resize=2, release=3.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeLauncherAllocationRequest {
    pub grant: ContentGrant,
    pub opening: u64,
    pub output: ContentOutputId,
    pub request_id: u64,
    pub prior: ContentAllocationId,
    pub operation: u16,
    pub edge: u16,
    pub desired_width: u32,
    pub desired_height: u32,
    pub margins: ContentMargins,
}

/// This replaces ContentCandidateBegin for the native launcher role. It owns
/// the row binding in the same candidate assembly, not in a parallel transaction.
/// Existing CandidateEnd and candidate outcomes complete this transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeLauncherCandidateBegin {
    pub content: ContentCandidateBegin,
    pub opening: u64,
    pub catalog_generation: u64,
    pub state_revision: u64,
    pub selected: u16,
    pub rows: Vec<u16>,
}

/// Exact presented authority. None of these fields may be filled from a newer
/// snapshot while acknowledging an older event. The lease is Session-minted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeLauncherBinding {
    pub grant: ContentGrant,
    pub opening: u64,
    pub output: ContentOutputId,
    pub allocation: ContentAllocationId,
    pub catalog_generation: u64,
    pub candidate_generation: u64,
    pub presentation_epoch: u64,
    pub interaction_generation: u64,
    pub state_revision: u64,
    pub focus_lease: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeLauncherFocusRevoked {
    pub binding: NativeLauncherBinding,
    pub reason: u16,
}

/// Event IDs are unique within their cause family and connection. Activation
/// carries its cause kind explicitly; content-action and keyboard IDs can overlap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeLauncherEvent {
    pub binding: NativeLauncherBinding,
    pub event_id: u64,
    pub state_revision: u64,
}

/// Semantic commands, not raw keycodes, X events, or a clipboard/helper channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum NativeLauncherInputKind {
    Text = 1,
    Left = 2,
    Right = 3,
    Home = 4,
    End = 5,
    Backspace = 6,
    Delete = 7,
    Previous = 8,
    Next = 9,
    PagePrevious = 10,
    PageNext = 11,
    First = 12,
    Last = 13,
    DeleteToStart = 14,
    DeleteToEnd = 15,
    DeleteWord = 16,
    Accept = 17,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeLauncherInput {
    pub event: NativeLauncherEvent,
    pub issued_mono_usec: u64,
    pub kind: NativeLauncherInputKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeLauncherInputAck {
    pub event: NativeLauncherEvent,
    /// Consumed=1, rejected-stale=2; independent of launch admission.
    pub disposition: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeLauncherActivation {
    pub event: NativeLauncherEvent,
    /// Keyboard Accept=1, presented ContentAction=2.
    pub cause: u16,
    pub slot: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeLauncherActivationOutcome {
    pub activation: NativeLauncherActivation,
    /// Admitted=1, stale=2, unknown=3, unauthorized=4, capacity=5.
    /// Admitted describes the Session policy queue, not application startup.
    pub status: u16,
    pub reason: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeLauncherClosed {
    pub grant: ContentGrant,
    pub opening: u64,
    pub reason: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellNativeLauncherRecord {
    Opening(NativeLauncherOpening),
    AllocationRequest(NativeLauncherAllocationRequest),
    CandidateBegin(NativeLauncherCandidateBegin),
    CandidateChunk(ContentCandidateChunk),
    Focus(NativeLauncherBinding),
    FocusRevoked(NativeLauncherFocusRevoked),
    Input(NativeLauncherInput),
    InputAck(NativeLauncherInputAck),
    Activate(NativeLauncherActivation),
    ActivationOutcome(NativeLauncherActivationOutcome),
    Closed(NativeLauncherClosed),
}
