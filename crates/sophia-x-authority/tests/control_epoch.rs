use sophia_input_authority::{
    AuthorityInstance, Capacity, ConnectionIdentity, InstanceId, IssuerHandle, SeatBinding,
};
use sophia_protocol::{DeviceId, SeatId};
use sophia_x_authority::{
    ControlEpochCoordinator, ControlEpochRefusal, TransitionInstallation, TransitionKind,
    next_transition_identity,
};

/// A coordinator starts where its authority starts.
///
/// A fresh AuthorityInstance is at epoch 0 and publication 0, so a coordinator
/// claiming any other starting point describes a world that does not exist,
/// and every transition it then requests is read by the authority as a much
/// larger jump than the test intended.
fn derive_coordinator(
    instance: &AuthorityInstance,
    issuer: &IssuerHandle,
) -> ControlEpochCoordinator {
    ControlEpochCoordinator::derive(instance, issuer).expect("a published revision to derive from")
}

fn authority() -> (AuthorityInstance, IssuerHandle) {
    let binding = SeatBinding::new(InstanceId::new(1), SeatId::from_raw(1));
    let (instance, issuer, _submit) =
        AuthorityInstance::new(binding, Capacity::PLANNED, 9).expect("planned capacity");
    (instance, issuer)
}

fn everything_cleared() -> TransitionInstallation {
    TransitionInstallation::security_control()
}

/// Drive one full transition to the given epoch and publication.
fn transition(
    coordinator: &mut ControlEpochCoordinator,
    instance: &mut AuthorityInstance,
    issuer: &IssuerHandle,
    epoch: u64,
    publication: u64,
) {
    let token = coordinator
        .request(
            instance,
            issuer,
            TransitionKind::SecurityControl,
            epoch,
            publication,
        )
        .expect("the transition to be requested");
    coordinator
        .apply(token, everything_cleared())
        .expect("the transition to apply");
    coordinator
        .reopen(instance, issuer, publication)
        .expect("the transition to reopen");
}

#[test]
fn the_applied_epoch_takes_the_exact_requested_value_however_far_it_jumped() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);

    transition(&mut coordinator, &mut instance, &issuer, 40, 2);

    // A counter that advanced independently would say 2 here and name a
    // transition that never happened.
    assert_eq!(coordinator.applied_control_epoch(), 40);
    assert_eq!(coordinator.requested_control_epoch(), 40);
}

#[test]
fn work_is_refused_while_a_transition_has_not_published() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    let before = coordinator.stamp().expect("an open coordinator to stamp");

    coordinator
        .request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            2,
            2,
        )
        .expect("the transition to be requested");

    // Not a fallback to the last applied values: that world was revoked the
    // moment routing closed.
    assert_eq!(
        coordinator.stamp(),
        Err(ControlEpochRefusal::TransitionPending)
    );
    assert_eq!(
        coordinator.admits(before),
        Err(ControlEpochRefusal::TransitionPending)
    );
}

#[test]
fn a_transition_that_cleared_only_some_populations_has_not_applied() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    let token = coordinator
        .request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            2,
            2,
        )
        .expect("the transition to be requested");

    let complete = TransitionInstallation::security_control();
    for partial in [
        TransitionInstallation {
            x_grabs_cleared: false,
            ..complete
        },
        TransitionInstallation {
            pointer_state_cleared: false,
            ..complete
        },
        TransitionInstallation {
            frozen_input_cleared: false,
            ..complete
        },
        TransitionInstallation {
            snapshot_installed: false,
            ..complete
        },
    ] {
        assert_eq!(
            coordinator.apply(token, partial),
            Err(ControlEpochRefusal::NotInstalled {
                kind: TransitionKind::SecurityControl
            })
        );
        assert_eq!(coordinator.applied_control_epoch(), 0);
    }
}

#[test]
fn routing_does_not_reopen_before_the_transition_applied() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    coordinator
        .request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            2,
            2,
        )
        .expect("the transition to be requested");

    assert_eq!(
        coordinator.reopen(&mut instance, &issuer, 2),
        Err(ControlEpochRefusal::NotInstalled {
            kind: TransitionKind::SecurityControl
        })
    );
    assert!(!coordinator.is_open());
}

#[test]
fn routing_reopens_only_on_the_publication_the_transition_was_requested_under() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    let token = coordinator
        .request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            2,
            7,
        )
        .expect("the transition to be requested");
    coordinator
        .apply(token, everything_cleared())
        .expect("the transition to apply");

    assert_eq!(
        coordinator.reopen(&mut instance, &issuer, 6),
        Err(ControlEpochRefusal::PublicationMismatch {
            stamped: 6,
            committed: 7,
        })
    );
    assert!(!coordinator.is_open());

    coordinator
        .reopen(&mut instance, &issuer, 7)
        .expect("the matching publication to reopen");
    assert!(coordinator.is_open());
}

#[test]
fn a_stamp_from_before_a_transition_is_not_admitted_after_it() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    let stale = coordinator.stamp().expect("an open coordinator to stamp");

    transition(&mut coordinator, &mut instance, &issuer, 2, 2);

    assert_eq!(
        coordinator.admits(stale),
        Err(ControlEpochRefusal::EpochMismatch {
            stamped: 0,
            applied: 2,
        })
    );
    let fresh = coordinator.stamp().expect("the reopened coordinator");
    assert_eq!(coordinator.admits(fresh), Ok(()));
}

#[test]
fn a_stamp_naming_another_publication_is_not_admitted() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    transition(&mut coordinator, &mut instance, &issuer, 2, 5);

    let mut forged = coordinator.stamp().expect("the reopened coordinator");
    forged.publication = 4;

    assert_eq!(
        coordinator.admits(forged),
        Err(ControlEpochRefusal::PublicationMismatch {
            stamped: 4,
            committed: 5,
        })
    );
}

#[test]
fn a_requested_epoch_that_does_not_move_forward_is_refused() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    transition(&mut coordinator, &mut instance, &issuer, 5, 2);

    for backwards in [4, 1] {
        assert_eq!(
            coordinator.request(
                &mut instance,
                &issuer,
                TransitionKind::SecurityControl,
                backwards,
                3
            ),
            Err(ControlEpochRefusal::EpochWentBackwards {
                requested: backwards,
                current: 5,
            })
        );
        // The refusal left routing open rather than closing it on a request
        // that was never valid.
        assert!(coordinator.is_open());
    }

    // Standing still is its own refusal, not a backwards step.
    assert_eq!(
        coordinator.request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            5,
            3
        ),
        Err(ControlEpochRefusal::EpochUnchanged { epoch: 5 })
    );
    assert!(coordinator.is_open());
}

#[test]
fn the_stamp_survives_the_round_trip_it_is_meant_to_survive() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    transition(&mut coordinator, &mut instance, &issuer, 9, 3);

    // Queue and thaw carry this pair unchanged; nothing restamps on the way
    // out, so what executes is what was admitted.
    let queued = coordinator.stamp().expect("the reopened coordinator");
    assert_eq!(queued.control_epoch, 9);
    assert_eq!(queued.publication, 3);
    assert_eq!(coordinator.admits(queued), Ok(()));
}

#[test]
fn an_ordinary_focus_publication_keeps_the_epoch_and_does_not_revoke_grabs() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    transition(&mut coordinator, &mut instance, &issuer, 4, 1);

    let token = coordinator
        .request(&mut instance, &issuer, TransitionKind::Publication, 4, 2)
        .expect("a publication-only transition to be requested");
    // No grab clearing is reported, because a focus change must not revoke
    // what the epoch still authorises.
    coordinator
        .apply(token, TransitionInstallation::publication())
        .expect("the snapshot to install");
    coordinator
        .reopen(&mut instance, &issuer, 2)
        .expect("the publication to reopen routing");

    assert_eq!(coordinator.applied_control_epoch(), 4);
    assert_eq!(coordinator.committed_publication(), 2);
}

#[test]
fn a_publication_only_transition_cannot_reopen_before_it_installed() {
    let (mut instance, issuer) = authority();
    // The epoch never moves here, so proving application by epoch equality
    // would be satisfied before this transition did anything at all.
    let mut coordinator = derive_coordinator(&instance, &issuer);
    transition(&mut coordinator, &mut instance, &issuer, 4, 1);
    coordinator
        .request(&mut instance, &issuer, TransitionKind::Publication, 4, 2)
        .expect("a publication-only transition to be requested");

    assert_eq!(
        coordinator.reopen(&mut instance, &issuer, 2),
        Err(ControlEpochRefusal::NotInstalled {
            kind: TransitionKind::Publication
        })
    );
    assert!(!coordinator.is_open());
}

#[test]
fn the_initial_epoch_does_not_let_a_transition_reopen_without_installing() {
    let (mut instance, issuer) = authority();
    // Starting at zero, where requested and applied are equal from the very
    // beginning, is the case that slipped through before.
    let mut coordinator = derive_coordinator(&instance, &issuer);
    coordinator
        .request(&mut instance, &issuer, TransitionKind::Publication, 0, 1)
        .expect("a publication-only transition to be requested");

    assert_eq!(
        coordinator.reopen(&mut instance, &issuer, 1),
        Err(ControlEpochRefusal::NotInstalled {
            kind: TransitionKind::Publication
        })
    );
}

#[test]
fn a_security_transition_must_advance_the_epoch() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    transition(&mut coordinator, &mut instance, &issuer, 4, 1);

    // Keeping the epoch would revoke grabs while work stamped with the old
    // epoch stayed admissible.
    assert_eq!(
        coordinator.request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            4,
            2
        ),
        Err(ControlEpochRefusal::EpochUnchanged { epoch: 4 })
    );
    assert!(coordinator.is_open());
}

#[test]
fn a_publication_only_transition_must_not_move_the_epoch() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    transition(&mut coordinator, &mut instance, &issuer, 4, 1);

    // Otherwise it is a security change that skipped revocation.
    assert_eq!(
        coordinator.request(&mut instance, &issuer, TransitionKind::Publication, 5, 2),
        Err(ControlEpochRefusal::EpochWentBackwards {
            requested: 5,
            current: 4,
        })
    );
    assert!(coordinator.is_open());
}

#[test]
fn a_publication_transition_is_not_satisfied_by_security_clearing_alone() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    transition(&mut coordinator, &mut instance, &issuer, 4, 1);
    let token = coordinator
        .request(&mut instance, &issuer, TransitionKind::Publication, 4, 2)
        .expect("a publication-only transition to be requested");

    // Grabs cleared but no snapshot installed is not this transition's proof.
    assert_eq!(
        coordinator.apply(
            token,
            TransitionInstallation {
                x_grabs_cleared: true,
                pointer_state_cleared: true,
                frozen_input_cleared: true,
                snapshot_installed: false,
            }
        ),
        Err(ControlEpochRefusal::NotInstalled {
            kind: TransitionKind::Publication
        })
    );
}

#[test]
fn applying_without_a_requested_transition_is_refused() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    let spent = coordinator
        .request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            1,
            1,
        )
        .expect("the transition to be requested");
    coordinator
        .apply(spent, everything_cleared())
        .expect("the transition to apply");
    coordinator
        .reopen(&mut instance, &issuer, 1)
        .expect("the transition to reopen");

    // The token outlives the transition it named, and must not resurrect it.
    assert_eq!(
        coordinator.apply(spent, TransitionInstallation::security_control()),
        Err(ControlEpochRefusal::NoTransition)
    );
}

#[test]
fn a_granted_capability_survives_an_ordinary_focus_publication() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    let connection = ConnectionIdentity {
        recipient: 7,
        connection_generation: 1,
    };
    let (grant, generation) = instance
        .issue_grant(&issuer, connection)
        .expect("a grant to be issued");

    let token = coordinator
        .request(&mut instance, &issuer, TransitionKind::Publication, 0, 1)
        .expect("a publication-only transition to be requested");
    coordinator
        .apply(token, TransitionInstallation::publication())
        .expect("the snapshot to install");
    coordinator
        .reopen(&mut instance, &issuer, 1)
        .expect("the publication to reopen routing");

    // The whole point of separating the kinds: a focus change must not cost a
    // client the capability it was granted.
    instance
        .allocate_device(&issuer, grant, generation, DeviceId::from_raw(1))
        .expect("the granted capability to survive the publication");
}

#[test]
fn a_security_transition_does_revoke_the_granted_capability() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    let connection = ConnectionIdentity {
        recipient: 7,
        connection_generation: 1,
    };
    let (grant, generation) = instance
        .issue_grant(&issuer, connection)
        .expect("a grant to be issued");

    transition(&mut coordinator, &mut instance, &issuer, 1, 1);

    // The counterpart, so the survivor above is evidence of a distinction
    // rather than of nothing ever being revoked.
    assert!(
        instance
            .allocate_device(&issuer, grant, generation, DeviceId::from_raw(1))
            .is_err(),
        "a security transition must revoke what the old epoch authorised"
    );
}

#[test]
fn a_second_transition_cannot_supersede_one_in_flight() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    coordinator
        .request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            1,
            1,
        )
        .expect("the first transition to be requested");

    // Superseding would leave the first transition's clearing half done and an
    // installation report in the air with nothing to match it against.
    assert_eq!(
        coordinator.request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            2,
            2
        ),
        Err(ControlEpochRefusal::TransitionPending)
    );
}

#[test]
fn an_installation_report_cannot_be_filed_against_a_later_transition() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    let first = coordinator
        .request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            1,
            1,
        )
        .expect("the first transition to be requested");
    coordinator
        .apply(first, everything_cleared())
        .expect("the first transition to apply");
    coordinator
        .reopen(&mut instance, &issuer, 1)
        .expect("the first transition to reopen");

    coordinator
        .request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            2,
            2,
        )
        .expect("the second transition to be requested");

    // A report prepared for the first transition arriving late must not count
    // as the second transition having cleared anything.
    assert_eq!(
        coordinator.apply(first, everything_cleared()),
        Err(ControlEpochRefusal::WrongTransition)
    );
    assert!(!coordinator.is_open());
}

#[test]
fn transition_identities_are_refused_rather_than_reused_when_exhausted() {
    // Saturating would hand out an identity an earlier installation still
    // answers to, which is the aliasing the token exists to prevent.
    assert_eq!(
        next_transition_identity(u64::MAX),
        Err(ControlEpochRefusal::TransitionIdentitiesExhausted)
    );
    assert_eq!(next_transition_identity(41), Ok(42));
}

#[test]
fn a_token_from_another_coordinator_is_not_this_ones_installation_identity() {
    let (mut instance, issuer) = authority();
    let (mut other_instance, other_issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    let mut other = derive_coordinator(&other_instance, &other_issuer);

    // Both are each coordinator's first transition, so a token that carried
    // only the transition number would be the same value in both.
    let foreign = other
        .request(
            &mut other_instance,
            &other_issuer,
            TransitionKind::SecurityControl,
            1,
            1,
        )
        .expect("the other transition to be requested");
    coordinator
        .request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            1,
            1,
        )
        .expect("this transition to be requested");

    assert_eq!(
        coordinator.apply(foreign, everything_cleared()),
        Err(ControlEpochRefusal::WrongTransition)
    );
    assert!(!coordinator.is_open());
}

#[test]
fn a_coordinator_cannot_be_derived_while_a_transition_is_pending() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    coordinator
        .request(
            &mut instance,
            &issuer,
            TransitionKind::SecurityControl,
            1,
            1,
        )
        .expect("the transition to be requested");

    // Mid-transition there is no published revision, and a second coordinator
    // attaching here must not start from the last good one.
    assert!(ControlEpochCoordinator::derive(&instance, &issuer).is_err());
}

#[test]
fn a_coordinator_derives_the_advanced_state_rather_than_resetting_to_zero() {
    let (mut instance, issuer) = authority();
    let mut coordinator = derive_coordinator(&instance, &issuer);
    transition(&mut coordinator, &mut instance, &issuer, 6, 3);

    // Attaching a fresh coordinator to an authority that already moved is the
    // seam a zero-argument constructor left open.
    let attached = ControlEpochCoordinator::derive(&instance, &issuer)
        .expect("the advanced revision to derive");
    assert_eq!(attached.applied_control_epoch(), 6);
    assert_eq!(attached.committed_publication(), 3);
}
