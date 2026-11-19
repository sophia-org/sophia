use crate::{NamespaceId, NamespacePortalCapability, PortalTransferId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortalTransfer {
    pub transfer: PortalTransferId,
    pub source_namespace: NamespaceId,
    pub target_namespace: NamespaceId,
    pub kind: PortalTransferKind,
    pub mime_type: Option<String>,
    pub byte_size: u64,
    pub decision: PortalDecision,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalTransferKind {
    Clipboard,
    DragAndDrop,
    FileHandoff,
    ScreenCapture,
    ScreenRecording,
    UriOpen,
    Notification,
}

impl PortalTransferKind {
    pub const fn capability(self) -> NamespacePortalCapability {
        match self {
            Self::Clipboard => NamespacePortalCapability::Clipboard,
            Self::DragAndDrop => NamespacePortalCapability::DragAndDrop,
            Self::FileHandoff => NamespacePortalCapability::FileHandoff,
            Self::ScreenCapture => NamespacePortalCapability::ScreenCapture,
            Self::ScreenRecording => NamespacePortalCapability::ScreenRecording,
            Self::UriOpen => NamespacePortalCapability::UriOpen,
            Self::Notification => NamespacePortalCapability::Notification,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalDecision {
    Pending,
    Allowed,
    Denied,
    Revoked,
}

/// A deadline-bound portal request presented to policy without its payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortalRequest {
    pub transfer: PortalTransfer,
    pub deadline_msec: u64,
}

/// The separately tracked authority created by an allowed portal request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortalGrant {
    pub transfer: PortalTransferId,
    pub source_namespace: NamespaceId,
    pub target_namespace: NamespaceId,
    pub kind: PortalTransferKind,
    pub source_generation: u64,
    pub broker_generation: u64,
    pub deadline_msec: u64,
    pub state: PortalGrantState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalGrantState {
    Active,
    Completed,
    Revoked,
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortalBrokerResponsePacket {
    pub transfer: PortalTransferId,
    pub decision: PortalBrokerResponseDecision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortalBrokerResponseDecision {
    Allowed(PortalGrant),
    Denied,
}

pub const SOPHIA_PORTAL_MAX_MIME_TYPE_LEN: usize = 255;
