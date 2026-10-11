use sophia_portal::{
    ClipboardPortal, ClipboardTarget, ClipboardTransferRequest, PortalCommand, PortalError,
};
use sophia_protocol::PortalTransferId;

use crate::{
    MAX_CLIPBOARD_TEXT_HANDOFF_BYTES, X_ATOM_NONE, XAtom, XResourceId, XSelectionMonitor,
    XTimestamp, XWindowTable,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XSelectionRequest {
    pub requestor: XResourceId,
    pub selection: XAtom,
    pub target: XAtom,
    pub target_name: String,
    pub property: XAtom,
    pub time: XTimestamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardSelectionPortalRequest {
    pub request: ClipboardTransferRequest,
    pub failure: ClipboardSelectionFailureRequest,
    pub property: XAtom,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClipboardSelectionRequestError {
    UnknownRequestorNamespace,
    UnknownSourceOwner,
    MissingSourceNamespace,
    /// Retained for stable runtime error decoding; normal dispatch now routes
    /// this case directly to the owning X11 client.
    SameNamespace,
    Portal(PortalError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClipboardSelectionDispatch {
    SameNamespace(ClipboardSelectionOwnerRequest),
    CrossNamespace {
        portal_request: ClipboardSelectionPortalRequest,
        command: PortalCommand,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardSelectionOwnerRequest {
    pub owner: XResourceId,
    pub requestor: XResourceId,
    pub selection: XAtom,
    pub target: XAtom,
    pub property: XAtom,
    pub time: XTimestamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingClipboardSelection {
    pub namespace: sophia_protocol::NamespaceId,
    pub portal_request: ClipboardSelectionPortalRequest,
    pub byte_order: crate::XByteOrder,
    pub external_source: bool,
}

/// Trusted owner registration, not an X selection owner or a grant. The
/// selected source adapter still verifies this generation at every handoff.
#[derive(Clone, Copy, Debug)]
pub struct XPortalSelectionOffer {
    pub source_namespace: sophia_protocol::NamespaceId,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardSelectionProxy {
    /// Bound by the frontend before the SelectionRequest is delivered.
    pub client: Option<u64>,
    pub transfer: PortalTransferId,
    pub namespace: sophia_protocol::NamespaceId,
    pub owner: XResourceId,
    pub requestor: XResourceId,
    pub selection: XAtom,
    pub target: XAtom,
    pub property: XAtom,
    pub time: XTimestamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardSourcePayload {
    pub transfer: PortalTransferId,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardSelectionExecutionError {
    UnknownTransfer,
    Denied,
    Expired,
    Disconnected,
    ExecutorFailure,
    StaleOwnerGeneration,
    UnsupportedTarget,
    InvalidUtf8,
    MissingProperty,
    PayloadTooLarge,
    Property,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClipboardSelectionExecutionOutcome {
    Handoff(ClipboardSelectionHandoff),
    Failed {
        error: ClipboardSelectionExecutionError,
        notify: ClipboardSelectionNotify,
    },
}

pub fn dispatch_clipboard_selection_request(
    request: XSelectionRequest,
    monitor: &XSelectionMonitor,
    windows: &XWindowTable,
    transfer: PortalTransferId,
    portal: &mut ClipboardPortal,
) -> Result<ClipboardSelectionDispatch, ClipboardSelectionRequestError> {
    let mut dispatch = prepare_clipboard_selection_request(request, monitor, windows, transfer)?;
    if let ClipboardSelectionDispatch::CrossNamespace {
        portal_request,
        command,
    } = &mut dispatch
    {
        *command = portal
            .request_import(portal_request.request.clone())
            .map_err(ClipboardSelectionRequestError::Portal)?;
    }
    Ok(dispatch)
}

pub(crate) fn prepare_clipboard_selection_request(
    request: XSelectionRequest,
    monitor: &XSelectionMonitor,
    windows: &XWindowTable,
    transfer: PortalTransferId,
) -> Result<ClipboardSelectionDispatch, ClipboardSelectionRequestError> {
    let requestor_namespace = windows
        .get(request.requestor)
        .ok_or(ClipboardSelectionRequestError::UnknownRequestorNamespace)?
        .namespace;
    let source_owner = monitor
        .owner(request.selection, Some(requestor_namespace))
        .filter(|record| record.owner.is_some())
        .or_else(|| monitor.current_owner_for_selection(request.selection))
        .ok_or(ClipboardSelectionRequestError::UnknownSourceOwner)?;
    let source_namespace = source_owner
        .namespace
        .ok_or(ClipboardSelectionRequestError::MissingSourceNamespace)?;
    if source_namespace == requestor_namespace {
        return Ok(ClipboardSelectionDispatch::SameNamespace(
            ClipboardSelectionOwnerRequest {
                owner: source_owner
                    .owner
                    .ok_or(ClipboardSelectionRequestError::UnknownSourceOwner)?,
                requestor: request.requestor,
                selection: request.selection,
                target: request.target,
                property: request.property,
                time: request.time,
            },
        ));
    }
    let portal_request =
        clipboard_portal_request_from_selection_request(request, monitor, windows, transfer)?;
    let command = PortalCommand::PromptClipboardTransfer(
        ClipboardPortal::prepare_import(&portal_request.request)
            .map_err(ClipboardSelectionRequestError::Portal)?,
    );

    Ok(ClipboardSelectionDispatch::CrossNamespace {
        portal_request,
        command,
    })
}

pub fn clipboard_portal_request_from_selection_request(
    request: XSelectionRequest,
    monitor: &XSelectionMonitor,
    windows: &XWindowTable,
    transfer: PortalTransferId,
) -> Result<ClipboardSelectionPortalRequest, ClipboardSelectionRequestError> {
    let requestor = windows
        .get(request.requestor)
        .ok_or(ClipboardSelectionRequestError::UnknownRequestorNamespace)?;
    let source_owner = monitor
        .current_owner_for_selection(request.selection)
        .ok_or(ClipboardSelectionRequestError::UnknownSourceOwner)?;
    let source_namespace = source_owner
        .namespace
        .ok_or(ClipboardSelectionRequestError::MissingSourceNamespace)?;

    if source_namespace == requestor.namespace {
        return Err(ClipboardSelectionRequestError::SameNamespace);
    }

    Ok(ClipboardSelectionPortalRequest {
        request: ClipboardTransferRequest {
            transfer,
            source_namespace,
            target_namespace: requestor.namespace,
            target: ClipboardTarget::Atom(request.target_name),
            byte_size: 0,
            generation: source_owner.generation,
        },
        failure: ClipboardSelectionFailureRequest {
            transfer,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            time: request.time,
        },
        property: request.property,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardSelectionFailureRequest {
    pub transfer: PortalTransferId,
    pub requestor: XResourceId,
    pub selection: XAtom,
    pub target: XAtom,
    pub time: XTimestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardSelectionNotify {
    pub time: XTimestamp,
    pub requestor: XResourceId,
    pub selection: XAtom,
    pub target: XAtom,
    pub property: XAtom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardSelectionFailure {
    pub transfer: PortalTransferId,
    pub notify: ClipboardSelectionNotify,
}

impl ClipboardSelectionFailure {
    pub fn failed_normally(&self) -> bool {
        self.notify.property == X_ATOM_NONE
    }
}

pub fn clipboard_selection_failure_notify(
    request: ClipboardSelectionFailureRequest,
) -> ClipboardSelectionFailure {
    ClipboardSelectionFailure {
        transfer: request.transfer,
        notify: ClipboardSelectionNotify {
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: X_ATOM_NONE,
        },
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardTextProperty {
    pub requestor: XResourceId,
    pub property: XAtom,
    pub target: XAtom,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardSelectionHandoff {
    pub transfer: PortalTransferId,
    pub property: ClipboardTextProperty,
    pub notify: ClipboardSelectionNotify,
}

impl ClipboardSelectionHandoff {
    pub fn succeeded_normally(&self) -> bool {
        self.notify.property == self.property.property && self.notify.property != X_ATOM_NONE
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClipboardSelectionHandoffError {
    NotHandoffCommand,
    TransferMismatch,
    MissingProperty,
    UnsupportedTarget,
    TextTooLarge { len: usize, max: usize },
}

pub fn clipboard_selection_text_handoff_artifact(
    command: &PortalCommand,
    request: &ClipboardSelectionPortalRequest,
    text: impl AsRef<str>,
) -> Result<ClipboardSelectionHandoff, ClipboardSelectionHandoffError> {
    let PortalCommand::HandoffClipboard { transfer } = command else {
        return Err(ClipboardSelectionHandoffError::NotHandoffCommand);
    };

    if *transfer != request.request.transfer {
        return Err(ClipboardSelectionHandoffError::TransferMismatch);
    }
    if request.property == X_ATOM_NONE {
        return Err(ClipboardSelectionHandoffError::MissingProperty);
    }
    if !request.request.target.is_text() {
        return Err(ClipboardSelectionHandoffError::UnsupportedTarget);
    }

    let bytes = text.as_ref().as_bytes();
    if bytes.len() > MAX_CLIPBOARD_TEXT_HANDOFF_BYTES {
        return Err(ClipboardSelectionHandoffError::TextTooLarge {
            len: bytes.len(),
            max: MAX_CLIPBOARD_TEXT_HANDOFF_BYTES,
        });
    }

    let failure = request.failure;
    Ok(ClipboardSelectionHandoff {
        transfer: *transfer,
        property: ClipboardTextProperty {
            requestor: failure.requestor,
            property: request.property,
            target: failure.target,
            bytes: bytes.to_vec(),
        },
        notify: ClipboardSelectionNotify {
            time: failure.time,
            requestor: failure.requestor,
            selection: failure.selection,
            target: failure.target,
            property: request.property,
        },
    })
}
