use std::path::Path;

use super::RealAtomicScanoutCard;
use crate::prelude::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RealAtomicScanoutCardReadiness {
    pub openable: bool,
    pub atomic_capable: bool,
    pub scanout_target: bool,
    pub atomic_properties: bool,
}

pub(crate) fn inspect_real_atomic_scanout_card(path: &Path) -> RealAtomicScanoutCardReadiness {
    let Ok(card) = RealAtomicScanoutCard::open_nonblocking(path) else {
        return RealAtomicScanoutCardReadiness::default();
    };
    inspect_opened_real_atomic_scanout_card(&card).readiness
}

pub(super) struct RealAtomicScanoutCardInspection {
    pub readiness: RealAtomicScanoutCardReadiness,
    pub selection: Option<LibdrmNativePrimaryPlaneSelection>,
}

pub(super) fn inspect_opened_real_atomic_scanout_card(
    card: &RealAtomicScanoutCard,
) -> RealAtomicScanoutCardInspection {
    let mut readiness = RealAtomicScanoutCardReadiness {
        openable: true,
        ..RealAtomicScanoutCardReadiness::default()
    };

    if !admit_atomic_scanout_client_capabilities(card) {
        return RealAtomicScanoutCardInspection {
            readiness,
            selection: None,
        };
    }
    readiness.atomic_capable = true;

    let target = select_native_primary_plane_target(card);
    if target.status != LibdrmNativePrimaryPlaneSelectionStatus::Selected {
        return RealAtomicScanoutCardInspection {
            readiness,
            selection: None,
        };
    }
    let Some(selection) = target.selection else {
        return RealAtomicScanoutCardInspection {
            readiness,
            selection: None,
        };
    };
    readiness.scanout_target = true;

    let properties = discover_native_primary_plane_property_handles(
        card,
        selection.connector,
        selection.crtc,
        selection.plane,
    );
    readiness.atomic_properties =
        properties.status == LibdrmNativePrimaryPlanePropertyDiscoveryStatus::Discovered;

    RealAtomicScanoutCardInspection {
        readiness,
        selection: readiness.atomic_properties.then_some(selection),
    }
}

pub(crate) fn admit_atomic_scanout_client_capabilities(card: &RealAtomicScanoutCard) -> bool {
    drm::Device::set_client_capability(card, drm::ClientCapability::UniversalPlanes, true).is_ok()
        && drm::Device::set_client_capability(card, drm::ClientCapability::Atomic, true).is_ok()
}
