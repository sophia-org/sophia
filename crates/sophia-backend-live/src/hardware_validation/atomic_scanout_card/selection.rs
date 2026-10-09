use std::path::Path;

use super::{
    RealAtomicScanoutCard, admit_atomic_scanout_client_capabilities,
    inspect_opened_real_atomic_scanout_card,
};
use crate::hardware_validation::preflight::nodes::is_primary_card_node_entry;
use crate::prelude::*;

#[derive(Debug)]
pub struct RealAtomicScanoutCardSelection {
    pub status: RealAtomicScanoutCardSelectionStatus,
    pub card: Option<RealAtomicScanoutCard>,
    pub selection: Option<LibdrmNativePrimaryPlaneSelection>,
}

impl RealAtomicScanoutCardSelection {
    pub(super) fn failed(status: RealAtomicScanoutCardSelectionStatus) -> Self {
        Self {
            status,
            card: None,
            selection: None,
        }
    }

    pub(super) fn selected(
        card: RealAtomicScanoutCard,
        selection: LibdrmNativePrimaryPlaneSelection,
    ) -> Self {
        Self {
            status: RealAtomicScanoutCardSelectionStatus::Selected,
            card: Some(card),
            selection: Some(selection),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealAtomicScanoutCardSelectionStatus {
    Selected,
    DeviceDirectoryUnavailable,
    NoPrimaryCardNodes,
    PrimaryCardOpenUnavailable,
    AtomicClientCapabilityUnavailable,
    KmsScanoutTargetUnavailable,
    AtomicPropertyDiscoveryUnavailable,
}

#[derive(Debug)]
pub struct RealAtomicScanoutCardTargetSet {
    pub card: RealAtomicScanoutCard,
    pub selections: Vec<LibdrmNativePrimaryPlaneSelection>,
}

#[derive(Debug)]
pub struct RealAtomicScanoutSelectionSet {
    pub status: RealAtomicScanoutSelectionSetStatus,
    pub cards: Vec<RealAtomicScanoutCardTargetSet>,
    pub connected_connectors: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealAtomicScanoutSelectionSetStatus {
    SelectedAll,
    DeviceDirectoryUnavailable,
    NoPrimaryCardNodes,
    NoCompleteTargets,
    Partial,
    CapacityExceeded,
    DeviceAdmissionUnavailable,
}

impl RealAtomicScanoutCardSelectionStatus {
    pub const fn failure_evidence(self) -> LibdrmNativeAtomicScanoutSmokeEvidence {
        match self {
            RealAtomicScanoutCardSelectionStatus::Selected => {
                LibdrmNativeAtomicScanoutSmokeEvidence::kms_selection_failed()
            }
            RealAtomicScanoutCardSelectionStatus::DeviceDirectoryUnavailable
            | RealAtomicScanoutCardSelectionStatus::NoPrimaryCardNodes => {
                LibdrmNativeAtomicScanoutSmokeEvidence::no_primary_card()
            }
            RealAtomicScanoutCardSelectionStatus::PrimaryCardOpenUnavailable => {
                LibdrmNativeAtomicScanoutSmokeEvidence::primary_card_open_failed()
            }
            RealAtomicScanoutCardSelectionStatus::AtomicClientCapabilityUnavailable => {
                LibdrmNativeAtomicScanoutSmokeEvidence::client_capability_failed()
            }
            RealAtomicScanoutCardSelectionStatus::KmsScanoutTargetUnavailable => {
                LibdrmNativeAtomicScanoutSmokeEvidence::kms_selection_failed()
            }
            RealAtomicScanoutCardSelectionStatus::AtomicPropertyDiscoveryUnavailable => {
                LibdrmNativeAtomicScanoutSmokeEvidence::property_discovery_failed()
            }
        }
    }
}

pub fn select_real_atomic_scanout_card() -> RealAtomicScanoutCardSelection {
    select_real_atomic_scanout_card_from_dev_dri(Path::new("/dev/dri"))
}

pub fn select_real_atomic_scanout_cards() -> RealAtomicScanoutSelectionSet {
    select_real_atomic_scanout_cards_from_dev_dri(Path::new("/dev/dri"))
}

#[cfg(feature = "seat-control")]
pub fn select_real_atomic_scanout_cards_with_seat(
    opener: &crate::LiveSeatDeviceOpener,
) -> RealAtomicScanoutSelectionSet {
    match crate::drm::seat_inventory::discover_admitted_seat_cards(
        opener.name(),
        opener.gpu_admission(),
    ) {
        Ok(cards) => select_real_atomic_scanout_cards_in_seat(opener, &cards),
        Err(_) => RealAtomicScanoutSelectionSet {
            status: RealAtomicScanoutSelectionSetStatus::DeviceAdmissionUnavailable,
            cards: Vec::new(),
            connected_connectors: 0,
        },
    }
}

#[cfg(feature = "seat-control")]
pub(crate) fn select_real_atomic_scanout_cards_in_seat(
    opener: &crate::LiveSeatDeviceOpener,
    cards: &[crate::drm::seat_inventory::SeatDrmCard],
) -> RealAtomicScanoutSelectionSet {
    select_real_atomic_scanout_candidates(
        cards.iter().map(|card| card.node.clone()).collect(),
        true,
        |path| {
            let admitted = cards
                .iter()
                .find(|card| card.node == path)
                .ok_or_else(|| io::Error::other("DRM card was not admitted"))?;
            admitted.validate_current(opener.name())?;
            let card = RealAtomicScanoutCard::open_admitted_with_seat(opener, admitted)?;
            admitted.validate_opened(&rustix::fs::fstat(&card)?)?;
            admitted.validate_current(opener.name())?;
            Ok(card)
        },
    )
}

pub fn select_real_atomic_scanout_cards_from_dev_dri(
    dev_dri: &Path,
) -> RealAtomicScanoutSelectionSet {
    select_real_atomic_scanout_cards_from_dev_dri_with(dev_dri, |path| {
        RealAtomicScanoutCard::open_nonblocking(path)
    })
}

fn select_real_atomic_scanout_cards_from_dev_dri_with(
    dev_dri: &Path,
    open: impl FnMut(&Path) -> io::Result<RealAtomicScanoutCard>,
) -> RealAtomicScanoutSelectionSet {
    let Ok(entries) = std::fs::read_dir(dev_dri) else {
        return RealAtomicScanoutSelectionSet {
            status: RealAtomicScanoutSelectionSetStatus::DeviceDirectoryUnavailable,
            cards: Vec::new(),
            connected_connectors: 0,
        };
    };
    let mut candidates = entries
        .filter_map(Result::ok)
        .filter(is_primary_card_node_entry)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    candidates.sort();
    select_real_atomic_scanout_candidates(candidates, false, open)
}

fn select_real_atomic_scanout_candidates(
    candidates: Vec<std::path::PathBuf>,
    require_every_open: bool,
    mut open: impl FnMut(&Path) -> io::Result<RealAtomicScanoutCard>,
) -> RealAtomicScanoutSelectionSet {
    if candidates.is_empty() {
        return RealAtomicScanoutSelectionSet {
            status: RealAtomicScanoutSelectionSetStatus::NoPrimaryCardNodes,
            cards: Vec::new(),
            connected_connectors: 0,
        };
    }

    let mut cards = Vec::new();
    let mut connected_connectors = 0usize;
    let mut incomplete = false;
    for path in candidates {
        let Ok(card) = open(&path) else {
            if require_every_open {
                return RealAtomicScanoutSelectionSet {
                    status: RealAtomicScanoutSelectionSetStatus::DeviceAdmissionUnavailable,
                    cards,
                    connected_connectors,
                };
            }
            continue;
        };
        if !admit_atomic_scanout_client_capabilities(&card) {
            incomplete = true;
            continue;
        }
        let selected = select_native_primary_plane_targets(&card);
        connected_connectors = connected_connectors.saturating_add(selected.connected_connectors);
        if connected_connectors > LIVE_RENDERED_OUTPUT_CAPACITY {
            return RealAtomicScanoutSelectionSet {
                status: RealAtomicScanoutSelectionSetStatus::CapacityExceeded,
                cards,
                connected_connectors,
            };
        }
        if selected.status != LibdrmNativePrimaryPlaneSelectionSetStatus::SelectedAll {
            incomplete |= selected.connected_connectors > 0;
        }
        let selections = selected
            .selections
            .into_iter()
            .filter(|selection| {
                discover_native_primary_plane_property_handles(
                    &card,
                    selection.connector,
                    selection.crtc,
                    selection.plane,
                )
                .status
                    == LibdrmNativePrimaryPlanePropertyDiscoveryStatus::Discovered
            })
            .collect::<Vec<_>>();
        incomplete |= selections.len() < selected.connected_connectors;
        if !selections.is_empty() {
            cards.push(RealAtomicScanoutCardTargetSet { card, selections });
        }
    }
    let selected_count = cards
        .iter()
        .map(|card| card.selections.len())
        .sum::<usize>();
    let status = if selected_count == 0 {
        RealAtomicScanoutSelectionSetStatus::NoCompleteTargets
    } else if incomplete || selected_count != connected_connectors {
        RealAtomicScanoutSelectionSetStatus::Partial
    } else {
        RealAtomicScanoutSelectionSetStatus::SelectedAll
    };
    RealAtomicScanoutSelectionSet {
        status,
        cards,
        connected_connectors,
    }
}

#[path = "../../../tests/support/seat_scanout_selection.rs"]
mod tests;

pub fn select_real_atomic_scanout_card_from_dev_dri(
    dev_dri: &Path,
) -> RealAtomicScanoutCardSelection {
    let Ok(entries) = std::fs::read_dir(dev_dri) else {
        return RealAtomicScanoutCardSelection::failed(
            RealAtomicScanoutCardSelectionStatus::DeviceDirectoryUnavailable,
        );
    };
    let mut candidates = entries
        .filter_map(Result::ok)
        .filter(is_primary_card_node_entry)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return RealAtomicScanoutCardSelection::failed(
            RealAtomicScanoutCardSelectionStatus::NoPrimaryCardNodes,
        );
    }
    candidates.sort();

    let mut saw_openable = false;
    let mut saw_atomic_capable = false;
    let mut saw_scanout_target = false;

    for path in candidates {
        let Ok(card) = RealAtomicScanoutCard::open_nonblocking(&path) else {
            continue;
        };
        let inspection = inspect_opened_real_atomic_scanout_card(&card);
        saw_openable |= inspection.readiness.openable;
        saw_atomic_capable |= inspection.readiness.atomic_capable;
        saw_scanout_target |= inspection.readiness.scanout_target;

        let Some(selection) = inspection.selection else {
            continue;
        };
        return RealAtomicScanoutCardSelection::selected(card, selection);
    }

    RealAtomicScanoutCardSelection::failed(if !saw_openable {
        RealAtomicScanoutCardSelectionStatus::PrimaryCardOpenUnavailable
    } else if !saw_atomic_capable {
        RealAtomicScanoutCardSelectionStatus::AtomicClientCapabilityUnavailable
    } else if !saw_scanout_target {
        RealAtomicScanoutCardSelectionStatus::KmsScanoutTargetUnavailable
    } else {
        RealAtomicScanoutCardSelectionStatus::AtomicPropertyDiscoveryUnavailable
    })
}
