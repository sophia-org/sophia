#![cfg(test)]

use super::*;

#[test]
fn an_admitted_open_failure_stops_selection_without_falling_back_to_another_card() {
    let candidates = vec!["/dev/dri/card1".into(), "/dev/dri/card3".into()];
    let mut opened = Vec::new();
    let result = select_real_atomic_scanout_candidates(candidates, true, |path| {
        opened.push(path.to_owned());
        Err(io::Error::other("admitted card could not be opened"))
    });
    assert_eq!(
        result.status,
        RealAtomicScanoutSelectionSetStatus::DeviceAdmissionUnavailable
    );
    assert_eq!(opened, vec![std::path::PathBuf::from("/dev/dri/card1")]);
    assert!(result.cards.is_empty());
}

#[test]
fn standalone_card_probes_keep_their_existing_best_effort_opening() {
    let candidates = vec!["/dev/dri/card1".into(), "/dev/dri/card3".into()];
    let mut opened = Vec::new();
    let result = select_real_atomic_scanout_candidates(candidates.clone(), false, |path| {
        opened.push(path.to_owned());
        Err(io::Error::other("probe device unavailable"))
    });
    assert_eq!(
        result.status,
        RealAtomicScanoutSelectionSetStatus::NoCompleteTargets
    );
    assert_eq!(opened, candidates);
}
