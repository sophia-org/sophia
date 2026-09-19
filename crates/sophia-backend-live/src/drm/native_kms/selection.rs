use crate::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibdrmNativePrimaryPlaneSelectionResult {
    pub status: LibdrmNativePrimaryPlaneSelectionStatus,
    pub selection: Option<LibdrmNativePrimaryPlaneSelection>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibdrmNativePrimaryPlaneSelectionStatus {
    Selected,
    ReadFailed,
    NoConnectedConnector,
    NoUsableMode,
    NoUsableEncoder,
    NoCompatibleCrtc,
    NoCompatiblePrimaryPlane,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibdrmNativePrimaryPlaneSelection {
    pub(crate) connector: drm::control::connector::Handle,
    pub(crate) crtc: drm::control::crtc::Handle,
    pub(crate) plane: drm::control::plane::Handle,
    /// The cursor plane on this CRTC, when the card offers one.
    ///
    /// Optional because a card need not have one and because every existing
    /// caller composing a selection by hand predates it. Discovery attaches
    /// it; nothing yet commits it.
    pub(crate) cursor: Option<drm::control::plane::Handle>,
    pub(crate) size: Size,
    pub(crate) mode: Option<drm::control::Mode>,
}

impl LibdrmNativePrimaryPlaneSelection {
    /// Builds a selection from raw KMS object handles.
    ///
    /// Discovery produces these in the ordinary path; this exists so a caller
    /// composing a head set by hand can too, the same way
    /// `LibdrmNativePrimaryPlaneObjects::new` already allows.
    pub const fn new(
        connector: drm::control::connector::Handle,
        crtc: drm::control::crtc::Handle,
        plane: drm::control::plane::Handle,
        size: Size,
        mode: Option<drm::control::Mode>,
    ) -> Self {
        Self {
            connector,
            crtc,
            plane,
            cursor: None,
            size,
            mode,
        }
    }

    /// The same selection with its CRTC's cursor plane attached.
    pub const fn with_cursor_plane(mut self, cursor: drm::control::plane::Handle) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// The cursor plane discovery found for this CRTC, if any.
    pub const fn cursor_plane(self) -> Option<drm::control::plane::Handle> {
        self.cursor
    }

    pub const fn size(self) -> Size {
        self.size
    }

    /// The KMS mode discovery attached, when it attached one.
    ///
    /// Optional for the same reason the field is: a caller composing a
    /// selection by hand need not supply one. A consumer that needs the real
    /// refresh must read it here rather than from a sysfs record, which
    /// carries only a resolution and fabricates the rest.
    pub const fn mode(self) -> Option<drm::control::Mode> {
        self.mode
    }

    pub fn connector_id(self) -> u32 {
        self.connector.into()
    }

    /// The connector and CRTC as handles, for callers composing atomic requests.
    /// `connector_id`/`crtc_id` remain the right choice for evidence, where a
    /// stable number is wanted rather than a handle.
    pub const fn connector_handle(self) -> drm::control::connector::Handle {
        self.connector
    }

    pub const fn crtc_handle(self) -> drm::control::crtc::Handle {
        self.crtc
    }

    pub const fn plane_handle(self) -> drm::control::plane::Handle {
        self.plane
    }

    pub fn crtc_id(self) -> u32 {
        self.crtc.into()
    }

    pub fn plane_id(self) -> u32 {
        self.plane.into()
    }

    pub const fn crtc_route(self, slot: LibdrmNativeOutputSlot) -> LibdrmNativeCrtcRoute {
        LibdrmNativeCrtcRoute::new(self.crtc, slot)
    }

    pub const fn into_objects(
        self,
        framebuffer: drm::control::framebuffer::Handle,
        mode_blob: Option<u64>,
    ) -> LibdrmNativePrimaryPlaneObjects {
        LibdrmNativePrimaryPlaneObjects::new_with_optional_mode_blob(
            self.connector,
            self.crtc,
            self.plane,
            framebuffer,
            mode_blob,
            self.size,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibdrmNativePrimaryPlaneSelectionSetResult {
    pub status: LibdrmNativePrimaryPlaneSelectionSetStatus,
    pub connected_connectors: usize,
    pub selections: Vec<LibdrmNativePrimaryPlaneSelection>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibdrmNativePrimaryPlaneSelectionSetStatus {
    SelectedAll,
    Partial,
    ReadFailed,
    NoConnectedConnector,
    CapacityExceeded,
    NoDisjointAssignment,
}

pub fn select_native_primary_plane_targets<D>(
    device: &D,
) -> LibdrmNativePrimaryPlaneSelectionSetResult
where
    D: LibdrmNativeKmsSelectionDevice,
{
    let (Ok(mut connectors), Ok(mut crtcs), Ok(mut planes)) = (
        device.connector_handles(),
        device.crtc_handles(),
        device.plane_handles(),
    ) else {
        return selection_set_failure(LibdrmNativePrimaryPlaneSelectionSetStatus::ReadFailed, 0);
    };
    connectors.sort_by_key(|handle| u32::from(*handle));
    crtcs.sort_by_key(|handle| u32::from(*handle));
    planes.sort_by_key(|handle| u32::from(*handle));

    let mut connected_connectors = 0usize;
    let mut selections = Vec::new();
    let mut used_crtcs = Vec::new();
    let mut used_planes = Vec::new();
    for connector in connectors {
        let Ok(snapshot) = device.connector_snapshot(connector) else {
            return selection_set_failure(
                LibdrmNativePrimaryPlaneSelectionSetStatus::ReadFailed,
                connected_connectors,
            );
        };
        if !snapshot.connected {
            continue;
        }
        connected_connectors = connected_connectors.saturating_add(1);
        if connected_connectors > crate::runtime::LIVE_RENDERED_OUTPUT_CAPACITY {
            return LibdrmNativePrimaryPlaneSelectionSetResult {
                status: LibdrmNativePrimaryPlaneSelectionSetStatus::CapacityExceeded,
                connected_connectors,
                selections,
            };
        }
        let Some(size) = snapshot
            .mode_size
            .filter(|size| size.width > 0 && size.height > 0)
        else {
            continue;
        };

        let mut selected = None;
        for encoder in snapshot.ordered_encoders() {
            let Ok(encoder) = device.encoder_snapshot(encoder) else {
                return selection_set_failure(
                    LibdrmNativePrimaryPlaneSelectionSetStatus::ReadFailed,
                    connected_connectors,
                );
            };
            for crtc in encoder.ordered_crtcs() {
                if !crtcs.contains(&crtc) || used_crtcs.contains(&crtc) {
                    continue;
                }
                for plane in planes.iter().copied() {
                    if used_planes.contains(&plane) {
                        continue;
                    }
                    let Ok(plane_snapshot) = device.plane_snapshot(plane) else {
                        return selection_set_failure(
                            LibdrmNativePrimaryPlaneSelectionSetStatus::ReadFailed,
                            connected_connectors,
                        );
                    };
                    if !plane_snapshot.supports_crtc(crtc) {
                        continue;
                    }
                    let Ok(plane_type) = device.plane_type(plane) else {
                        return selection_set_failure(
                            LibdrmNativePrimaryPlaneSelectionSetStatus::ReadFailed,
                            connected_connectors,
                        );
                    };
                    if plane_type == Some(drm::control::PlaneType::Primary) {
                        selected = Some(LibdrmNativePrimaryPlaneSelection {
                            connector,
                            crtc,
                            plane,
                            cursor: None,
                            size,
                            mode: snapshot.native_mode,
                        });
                        break;
                    }
                }
                if selected.is_some() {
                    break;
                }
            }
            if selected.is_some() {
                break;
            }
        }
        if let Some(selection) = selected {
            // The cursor plane for the same CRTC, when the card has one to
            // spare. A card without one, or whose only cursor plane is
            // already serving another head, simply keeps the legacy path --
            // discovery reports what exists rather than requiring it.
            let cursor = match select_plane_for_crtc(
                device,
                &planes,
                selection.crtc,
                drm::control::PlaneType::Cursor,
                &used_planes,
            ) {
                Ok(cursor) => cursor,
                Err(()) => {
                    return selection_set_failure(
                        LibdrmNativePrimaryPlaneSelectionSetStatus::ReadFailed,
                        connected_connectors,
                    );
                }
            };
            let selection = match cursor {
                Some(cursor) => selection.with_cursor_plane(cursor),
                None => selection,
            };
            used_crtcs.push(selection.crtc);
            used_planes.push(selection.plane);
            if let Some(cursor) = selection.cursor {
                used_planes.push(cursor);
            }
            selections.push(selection);
        }
    }

    let status = if connected_connectors == 0 {
        LibdrmNativePrimaryPlaneSelectionSetStatus::NoConnectedConnector
    } else if selections.len() == connected_connectors {
        LibdrmNativePrimaryPlaneSelectionSetStatus::SelectedAll
    } else if selections.is_empty() {
        LibdrmNativePrimaryPlaneSelectionSetStatus::NoDisjointAssignment
    } else {
        LibdrmNativePrimaryPlaneSelectionSetStatus::Partial
    };
    LibdrmNativePrimaryPlaneSelectionSetResult {
        status,
        connected_connectors,
        selections,
    }
}

fn selection_set_failure(
    status: LibdrmNativePrimaryPlaneSelectionSetStatus,
    connected_connectors: usize,
) -> LibdrmNativePrimaryPlaneSelectionSetResult {
    LibdrmNativePrimaryPlaneSelectionSetResult {
        status,
        connected_connectors,
        selections: Vec::new(),
    }
}

pub fn select_native_primary_plane_target<D>(device: &D) -> LibdrmNativePrimaryPlaneSelectionResult
where
    D: LibdrmNativeKmsSelectionDevice,
{
    let (Ok(connectors), Ok(crtcs), Ok(planes)) = (
        device.connector_handles(),
        device.crtc_handles(),
        device.plane_handles(),
    ) else {
        return LibdrmNativePrimaryPlaneSelectionResult {
            status: LibdrmNativePrimaryPlaneSelectionStatus::ReadFailed,
            selection: None,
        };
    };

    let mut saw_connected = false;
    let mut saw_mode = false;
    let mut saw_encoder = false;
    let mut saw_crtc = false;

    for connector in connectors {
        let Ok(connector_snapshot) = device.connector_snapshot(connector) else {
            return LibdrmNativePrimaryPlaneSelectionResult {
                status: LibdrmNativePrimaryPlaneSelectionStatus::ReadFailed,
                selection: None,
            };
        };
        if !connector_snapshot.connected {
            continue;
        }
        saw_connected = true;
        let Some(size) = connector_snapshot.mode_size else {
            continue;
        };
        if size.width <= 0 || size.height <= 0 {
            continue;
        }
        saw_mode = true;

        for encoder in connector_snapshot.ordered_encoders() {
            saw_encoder = true;
            let Ok(encoder_snapshot) = device.encoder_snapshot(encoder) else {
                return LibdrmNativePrimaryPlaneSelectionResult {
                    status: LibdrmNativePrimaryPlaneSelectionStatus::ReadFailed,
                    selection: None,
                };
            };
            for crtc in encoder_snapshot.ordered_crtcs() {
                if !crtcs.contains(&crtc) {
                    continue;
                }
                saw_crtc = true;
                let plane = match select_primary_plane_for_crtc(device, &planes, crtc) {
                    Ok(Some(plane)) => plane,
                    Ok(None) => continue,
                    Err(()) => {
                        return LibdrmNativePrimaryPlaneSelectionResult {
                            status: LibdrmNativePrimaryPlaneSelectionStatus::ReadFailed,
                            selection: None,
                        };
                    }
                };
                let cursor = match select_plane_for_crtc(
                    device,
                    &planes,
                    crtc,
                    drm::control::PlaneType::Cursor,
                    &[plane],
                ) {
                    Ok(cursor) => cursor,
                    Err(()) => {
                        return LibdrmNativePrimaryPlaneSelectionResult {
                            status: LibdrmNativePrimaryPlaneSelectionStatus::ReadFailed,
                            selection: None,
                        };
                    }
                };
                return LibdrmNativePrimaryPlaneSelectionResult {
                    status: LibdrmNativePrimaryPlaneSelectionStatus::Selected,
                    selection: Some(LibdrmNativePrimaryPlaneSelection {
                        connector,
                        crtc,
                        plane,
                        cursor,
                        size,
                        mode: connector_snapshot.native_mode,
                    }),
                };
            }
        }
    }

    LibdrmNativePrimaryPlaneSelectionResult {
        status: if !saw_connected {
            LibdrmNativePrimaryPlaneSelectionStatus::NoConnectedConnector
        } else if !saw_mode {
            LibdrmNativePrimaryPlaneSelectionStatus::NoUsableMode
        } else if !saw_encoder {
            LibdrmNativePrimaryPlaneSelectionStatus::NoUsableEncoder
        } else if !saw_crtc {
            LibdrmNativePrimaryPlaneSelectionStatus::NoCompatibleCrtc
        } else {
            LibdrmNativePrimaryPlaneSelectionStatus::NoCompatiblePrimaryPlane
        },
        selection: None,
    }
}

fn select_primary_plane_for_crtc<D>(
    device: &D,
    planes: &[drm::control::plane::Handle],
    crtc: drm::control::crtc::Handle,
) -> Result<Option<drm::control::plane::Handle>, ()>
where
    D: LibdrmNativeKmsSelectionDevice,
{
    select_plane_for_crtc(device, planes, crtc, drm::control::PlaneType::Primary, &[])
}

/// The first plane of a given type this CRTC can drive, skipping any already
/// claimed.
///
/// One function for both plane types because the question is the same one --
/// which plane of this kind can this CRTC use -- and asking it twice in two
/// shapes is how the two answers drift apart.
fn select_plane_for_crtc<D>(
    device: &D,
    planes: &[drm::control::plane::Handle],
    crtc: drm::control::crtc::Handle,
    wanted: drm::control::PlaneType,
    claimed: &[drm::control::plane::Handle],
) -> Result<Option<drm::control::plane::Handle>, ()>
where
    D: LibdrmNativeKmsSelectionDevice,
{
    for plane in planes.iter().copied() {
        if claimed.contains(&plane) {
            continue;
        }
        let Ok(snapshot) = device.plane_snapshot(plane) else {
            return Err(());
        };
        if !snapshot.supports_crtc(crtc) {
            continue;
        }
        let Ok(plane_type) = device.plane_type(plane) else {
            return Err(());
        };
        if plane_type == Some(wanted) {
            return Ok(Some(plane));
        }
    }
    Ok(None)
}
