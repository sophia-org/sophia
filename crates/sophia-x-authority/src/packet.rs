use sophia_portal::PortalCommand;
use sophia_protocol::{
    AuthoritySurface, NamespaceId, PortalTransfer, PortalTransferId, Rect, Region,
    SurfaceConstraints, SurfaceId, SurfaceTransaction, TransactionId,
};

use crate::{
    ClipboardSelectionFailure, ClipboardSelectionOwnerRequest, ClipboardSelectionRequestError,
    XAtom, XAuthorityAccessError, XResourceId, XSelectionChangeKind, XTimestamp,
};

pub const X_AUTHORITY_MAX_TARGET_NAME_LEN: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XAuthorityRequestPacket {
    pub transaction: TransactionId,
    pub namespace: NamespaceId,
    pub kind: XAuthorityRequestKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XAuthorityRequestKind {
    CreateWindow {
        window: XResourceId,
        surface: SurfaceId,
        geometry: Rect,
        constraints: SurfaceConstraints,
        generation: u64,
    },
    MapWindow {
        window: XResourceId,
        generation: u64,
    },
    PresentPixmap {
        window: XResourceId,
        pixmap: u32,
        damage: Region,
        previous_committed_generation: u64,
        timeout_msec: u32,
    },
    SetSelectionOwner {
        selection: XAtom,
        owner: Option<XResourceId>,
        timestamp: XTimestamp,
        selection_timestamp: XTimestamp,
        kind: XSelectionChangeKind,
    },
    RequestSelection {
        requestor: XResourceId,
        selection: XAtom,
        target: XAtom,
        target_name: String,
        property: XAtom,
        time: XTimestamp,
        transfer: PortalTransferId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XAuthorityResponsePacket {
    pub transaction: TransactionId,
    pub outcome: XAuthorityResponseOutcome,
    pub surfaces: Vec<AuthoritySurface>,
    /// Surfaces whose X11 window lifetime ended during this response.
    ///
    /// This is deliberately distinct from an unmapped `AuthoritySurface` so
    /// the Engine can remove its committed state rather than retain a stale
    /// visual.
    pub removed_surfaces: Vec<SurfaceId>,
    pub transactions: Vec<SurfaceTransaction>,
    pub portal_commands: Vec<XAuthorityPortalCommand>,
    pub selection_artifacts: Vec<XAuthoritySelectionArtifact>,
}

impl XAuthorityResponsePacket {
    pub fn accepted(transaction: TransactionId) -> Self {
        Self {
            transaction,
            outcome: XAuthorityResponseOutcome::Accepted,
            surfaces: Vec::new(),
            removed_surfaces: Vec::new(),
            transactions: Vec::new(),
            portal_commands: Vec::new(),
            selection_artifacts: Vec::new(),
        }
    }

    pub fn rejected(transaction: TransactionId, error: XAuthorityRuntimeError) -> Self {
        Self {
            transaction,
            outcome: XAuthorityResponseOutcome::Rejected(error),
            surfaces: Vec::new(),
            removed_surfaces: Vec::new(),
            transactions: Vec::new(),
            portal_commands: Vec::new(),
            selection_artifacts: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityResponseOutcome {
    Accepted,
    Rejected(XAuthorityRuntimeError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityRuntimeError {
    InvalidResource,
    InvalidNamespace,
    InvalidSurface,
    UnknownResource,
    WrongResourceKind,
    CrossNamespaceDenied,
    StaleGeneration,
    UnknownRequestorNamespace,
    UnknownSourceOwner,
    MissingSourceNamespace,
    SameNamespace,
    PortalRejected,
    /// A bound private focus producer could not establish its guarded state.
    FocusAuthorityUnavailable,
    /// An argument outside the range the protocol defines for it. This is not
    /// a bad resource: the client named something that is not a choice at all,
    /// such as a revert_to beyond Parent.
    InvalidValue,
    /// A real window that cannot serve the purpose it was named for because
    /// it is not viewable, meaning it or one of its ancestors is unmapped.
    WindowNotViewable,
}

impl From<XAuthorityAccessError> for XAuthorityRuntimeError {
    fn from(error: XAuthorityAccessError) -> Self {
        match error {
            XAuthorityAccessError::InvalidResource => Self::InvalidResource,
            XAuthorityAccessError::InvalidNamespace => Self::InvalidNamespace,
            XAuthorityAccessError::InvalidSurface => Self::InvalidSurface,
            XAuthorityAccessError::UnknownResource => Self::UnknownResource,
            XAuthorityAccessError::WrongResourceKind => Self::WrongResourceKind,
            XAuthorityAccessError::CrossNamespaceDenied => Self::CrossNamespaceDenied,
            XAuthorityAccessError::StaleGeneration => Self::StaleGeneration,
        }
    }
}

impl From<ClipboardSelectionRequestError> for XAuthorityRuntimeError {
    fn from(error: ClipboardSelectionRequestError) -> Self {
        match error {
            ClipboardSelectionRequestError::UnknownRequestorNamespace => {
                Self::UnknownRequestorNamespace
            }
            ClipboardSelectionRequestError::UnknownSourceOwner => Self::UnknownSourceOwner,
            ClipboardSelectionRequestError::MissingSourceNamespace => Self::MissingSourceNamespace,
            ClipboardSelectionRequestError::SameNamespace => Self::SameNamespace,
            ClipboardSelectionRequestError::Portal(_) => Self::PortalRejected,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XAuthorityPortalCommand {
    PromptClipboardTransfer(PortalTransfer),
    FailSelection { transfer: PortalTransferId },
    HandoffClipboard { transfer: PortalTransferId },
}

impl XAuthorityPortalCommand {
    pub fn from_portal_command(command: PortalCommand) -> Option<Self> {
        match command {
            PortalCommand::PromptClipboardTransfer(transfer) => {
                Some(Self::PromptClipboardTransfer(transfer))
            }
            PortalCommand::FailSelection { transfer } => Some(Self::FailSelection { transfer }),
            PortalCommand::HandoffClipboard { transfer } => {
                Some(Self::HandoffClipboard { transfer })
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XAuthoritySelectionArtifact {
    Failure(ClipboardSelectionFailure),
    Request(ClipboardSelectionOwnerRequest),
    Clear {
        owner: XResourceId,
        selection: XAtom,
        time: XTimestamp,
    },
}
