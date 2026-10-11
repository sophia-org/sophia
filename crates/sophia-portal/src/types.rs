use crate::prelude::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardTransferRequest {
    pub transfer: PortalTransferId,
    pub source_namespace: NamespaceId,
    pub target_namespace: NamespaceId,
    pub target: ClipboardTarget,
    pub byte_size: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardSourceOwnerChanged {
    pub source_namespace: NamespaceId,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DragAndDropTransferRequest {
    pub transfer: PortalTransferId,
    pub source_namespace: NamespaceId,
    pub target_namespace: NamespaceId,
    pub offered_types: Vec<String>,
    pub byte_size: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileHandoffRequest {
    pub transfer: PortalTransferId,
    pub source_namespace: NamespaceId,
    pub target_namespace: NamespaceId,
    pub mode: FileHandoffMode,
    pub offered_types: Vec<String>,
    pub suggested_name: Option<String>,
    pub byte_size: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenCaptureRequest {
    pub transfer: PortalTransferId,
    pub source_namespace: NamespaceId,
    pub target_namespace: NamespaceId,
    pub mode: ScreenCaptureMode,
    pub scope: ScreenCaptureScope,
    pub mime_type: String,
    pub byte_size: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UriOpenRequest {
    pub transfer: PortalTransferId,
    pub source_namespace: NamespaceId,
    pub target_namespace: NamespaceId,
    pub uri: String,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationRequest {
    pub transfer: PortalTransferId,
    pub source_namespace: NamespaceId,
    pub target_namespace: NamespaceId,
    pub summary: String,
    pub body: Option<String>,
    pub urgency: NotificationUrgency,
    pub actions: Vec<String>,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileHandoffMode {
    Open,
    Save,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScreenCaptureMode {
    Screenshot,
    ScreenRecording,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScreenCaptureScope {
    Desktop,
    Output,
    Surface,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationUrgency {
    Low,
    Normal,
    Critical,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClipboardTarget {
    Atom(String),
    Mime(String),
}

impl ClipboardTarget {
    pub fn as_str(&self) -> &str {
        match self {
            ClipboardTarget::Atom(target) | ClipboardTarget::Mime(target) => target,
        }
    }

    pub fn is_text(&self) -> bool {
        let target = self.as_str();
        let lower = target.to_ascii_lowercase();

        matches!(target, "UTF8_STRING" | "TEXT" | "STRING")
            || lower == "text/plain"
            || lower.starts_with("text/plain;")
            || lower.starts_with("text/")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortalCommand {
    PromptClipboardTransfer(PortalTransfer),
    HandoffClipboard { transfer: PortalTransferId },
    FailSelection { transfer: PortalTransferId },
    PromptDragAndDropTransfer(PortalTransfer),
    HandoffDragAndDrop { transfer: PortalTransferId },
    CancelDragAndDrop { transfer: PortalTransferId },
    PromptFileHandoff(PortalTransfer),
    HandoffFile { transfer: PortalTransferId },
    CancelFileHandoff { transfer: PortalTransferId },
    PromptScreenCapture(PortalTransfer),
    HandoffScreenCapture { transfer: PortalTransferId },
    CancelScreenCapture { transfer: PortalTransferId },
    PromptUriOpen(PortalTransfer),
    HandoffUriOpen { transfer: PortalTransferId },
    CancelUriOpen { transfer: PortalTransferId },
    PromptNotification(PortalTransfer),
    DeliverNotification { transfer: PortalTransferId },
    DropNotification { transfer: PortalTransferId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortalError {
    DuplicateTransfer,
    CapacityExceeded,
    InvalidTransfer,
    InvalidNamespace,
    UnsupportedTarget,
    MissingTransferType,
    TooManyTransferTypes,
    InvalidSuggestedName,
    UnsupportedCaptureMimeType,
    InvalidUri,
    UnsupportedUriScheme,
    InvalidNotificationText,
    TooManyNotificationActions,
    UnknownTransfer,
    NotPending,
}

pub const MAX_DRAG_AND_DROP_TYPES: usize = 16;
pub const MAX_FILE_HANDOFF_TYPES: usize = 32;
pub const MAX_SUGGESTED_FILE_NAME_LEN: usize = 255;
pub const MAX_URI_LEN: usize = 2048;
pub const MAX_NOTIFICATION_SUMMARY_LEN: usize = 128;
pub const MAX_NOTIFICATION_BODY_LEN: usize = 1024;
pub const MAX_NOTIFICATION_ACTIONS: usize = 4;
pub const MAX_NOTIFICATION_ACTION_LEN: usize = 64;
