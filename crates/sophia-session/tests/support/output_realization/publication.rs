#![cfg(test)]

use super::{binding, profile, realized};
use crate::live_output_authority::LiveOutputAuthorityOwner;
use crate::live_session::output_realization::{OutputRealizationLedger, PendingOutputPublication};
use crate::live_session::{LiveOutputTopologyOwner, LiveOutputTopologyRebuild};
use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
    project_live_output_authority_snapshot,
};
use sophia_engine::{HeadlessOutput, RenderHeadId};
use sophia_protocol::{OutputAuthoritySnapshot, OutputId, Size};

fn output() -> HeadlessOutput {
    HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 1920,
            height: 1080,
        },
        scale: 1,
    }
}

fn publication() -> PendingOutputPublication {
    let mode = LibdrmNativeOutputTiming::new(1920, 1080, 60_000);
    let capability = LibdrmNativeOutputCapability::new(
        output().id,
        1,
        "DP-2",
        [mode],
        Some(mode),
        mode,
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .unwrap()
    .bind_head(RenderHeadId::from_raw(1))
    .unwrap();
    let capabilities = vec![capability];
    PendingOutputPublication {
        binding: binding(1, 1, 2),
        snapshot: project_live_output_authority_snapshot(&capabilities, &[output()], 1).unwrap(),
        capabilities,
        already_published: false,
    }
}

fn owner() -> LiveOutputTopologyOwner {
    let mut owner =
        LiveOutputTopologyOwner::new_at_generation(vec![output()], vec![(output().id, 1)], 1)
            .unwrap();
    owner.begin_rescan(1).unwrap();
    // A period with no output must retain the last published topology.
    assert_eq!(
        owner.observe_rebuild(Vec::new(), Vec::new()).unwrap(),
        LiveOutputTopologyRebuild::Unavailable
    );
    owner
}

fn observe(
    owner: &mut LiveOutputTopologyOwner,
    replacement: &mut PendingOutputPublication,
    published: Option<&OutputAuthoritySnapshot>,
    realization_changed: bool,
) -> LiveOutputTopologyRebuild {
    owner
        .observe_publication_rebuild(
            vec![output()],
            vec![(output().id, 1)],
            realization_changed,
            replacement,
            published,
        )
        .unwrap()
}

#[test]
fn same_topology_return_commits_its_realization_after_presentation_without_republishing() {
    let mut owner = owner();
    let mut replacement = publication();
    let published = replacement.snapshot.clone();
    let mut ledger = OutputRealizationLedger::default();
    ledger.stage(binding(0, 0, 1), realized("DP-2")).unwrap();
    assert!(ledger.commit(binding(0, 0, 1), &profile()));
    ledger.stage(replacement.binding, realized("DP-2")).unwrap();

    assert_eq!(
        observe(&mut owner, &mut replacement, Some(&published), false),
        LiveOutputTopologyRebuild::TransportReplaced
    );
    assert_eq!(owner.topology_epoch, 1);
    assert_eq!(owner.publication_generation, 1);
    owner.mark_published(7, false).unwrap();
    assert!(!owner.observe_presentation(7));
    assert!(ledger.pending(replacement.binding, &profile()).is_some());
    assert!(owner.observe_presentation(8));
    assert!(
        !replacement.has_stale_epoch(Some(published.topology_epoch)),
        "equal-epoch transport replacement must reach ledger settlement"
    );
    assert!(
        replacement.already_published,
        "unchanged authority must not be republished"
    );
    assert!(ledger.commit(replacement.binding, &profile()));
    assert!(ledger.pending(replacement.binding, &profile()).is_none());
    assert!(!ledger.commit(binding(0, 0, 1), &profile()));
}

#[test]
fn same_geometry_with_new_capabilities_publishes_at_a_new_epoch() {
    for change in ["mode", "vrr", "mapping"] {
        let mut owner = owner();
        let mut replacement = publication();
        let published = replacement.snapshot.clone();
        let mut authority = LiveOutputAuthorityOwner::new(1, published.clone()).unwrap();
        match change {
            "mode" => {
                // The realized mode stays at 60 Hz; only the advertised list changes.
                let mut extra = replacement.snapshot.heads[0].modes[0].clone();
                extra.mode = sophia_protocol::DisplayModeId::from_raw(2);
                extra.refresh_millihz = 75_000;
                extra.preferred = false;
                replacement.snapshot.heads[0].modes.push(extra);
            }
            "vrr" => replacement.snapshot.heads[0].vrr_capable = true,
            "mapping" => {
                replacement.snapshot.groups[0].members[0].mapping =
                    sophia_protocol::OutputHeadMapping::Cover
            }
            _ => unreachable!(),
        }
        assert_eq!(
            observe(&mut owner, &mut replacement, Some(&published), false),
            LiveOutputTopologyRebuild::TopologyChanged,
            "{change}"
        );
        assert_eq!(owner.topology_epoch, 2);
        assert_eq!(owner.publication_generation, 2);
        assert_eq!(replacement.snapshot.topology_epoch, 2);
        assert!(!replacement.already_published);
        assert!(!replacement.has_stale_epoch(Some(published.topology_epoch)));
        assert_eq!(authority.published(), &published);
        owner.mark_published(7, false).unwrap();
        assert!(!owner.observe_presentation(7));
        assert!(owner.observe_presentation(8));
        authority
            .replace_published_snapshot(replacement.snapshot.clone())
            .unwrap();
        assert_eq!(authority.published(), &replacement.snapshot);
    }
}

#[test]
fn realization_change_still_advances_an_identical_public_snapshot() {
    let mut owner = owner();
    let mut replacement = publication();
    let published = replacement.snapshot.clone();
    assert_eq!(
        observe(&mut owner, &mut replacement, Some(&published), true),
        LiveOutputTopologyRebuild::TopologyChanged
    );
    assert_eq!(replacement.snapshot.topology_epoch, 2);
    assert!(!replacement.already_published);
}

#[test]
fn comparison_ignores_only_the_incoming_snapshot_epoch() {
    let mut owner = owner();
    let mut replacement = publication();
    let published = replacement.snapshot.clone();
    replacement.snapshot.topology_epoch = 99;
    assert_eq!(
        observe(&mut owner, &mut replacement, Some(&published), false),
        LiveOutputTopologyRebuild::TransportReplaced
    );
    assert!(replacement.already_published);
    assert_eq!(replacement.snapshot, published);
}

#[test]
fn an_absent_or_older_publication_cannot_be_treated_as_already_published() {
    for missing in [true, false] {
        let mut owner = owner();
        owner.topology_epoch = 2;
        let mut replacement = publication();
        let published = replacement.snapshot.clone();
        assert_eq!(
            observe(
                &mut owner,
                &mut replacement,
                (!missing).then_some(&published),
                false
            ),
            LiveOutputTopologyRebuild::TopologyChanged
        );
        assert_eq!(replacement.snapshot.topology_epoch, 3);
        assert!(!replacement.already_published);
    }
}

#[test]
fn an_already_published_settlement_still_refuses_an_absent_or_superseded_epoch() {
    let mut replacement = publication();
    replacement.already_published = true;
    for epoch in [None, Some(0), Some(2)] {
        assert!(replacement.has_stale_epoch(epoch));
    }
    assert!(!replacement.has_stale_epoch(Some(1)));
    replacement.already_published = false;
    assert!(
        replacement.has_stale_epoch(Some(1)),
        "a new publication still needs a newer epoch"
    );
}
