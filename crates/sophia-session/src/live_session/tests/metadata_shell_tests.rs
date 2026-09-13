use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn action(
    token: u64,
    issuer_epoch: u64,
    revocation_epoch: u64,
    generation: u64,
) -> sophia_protocol::ToplevelActionCapabilityRef {
    sophia_protocol::ToplevelActionCapabilityRef {
        token,
        issuer_epoch,
        issuer_revocation_epoch: revocation_epoch,
        recipient_epoch: 7,
        target_slot: 3,
        target_generation: generation,
    }
}

#[test]
fn broker_dispatch_requires_the_exact_current_issuer_tuple() {
    let surface = SurfaceId::new(41, 2);
    let mut descriptors = sophia_engine::ChromeDescriptorTable::default();
    descriptors.upsert(sophia_protocol::ChromeDescriptor {
        surface,
        label: Some(sophia_protocol::DisplayLabel {
            text: "Terminal".to_owned(),
            redacted: false,
        }),
        icon: None,
        trust_level: sophia_protocol::TrustLevel::Trusted,
        attention: sophia_protocol::AttentionState::None,
        generation: 9,
    });
    let grants = BTreeMap::from([(
        surface,
        sophia_protocol::BrokerToplevelActionGrant {
            token: 11,
            revocation_epoch: 5,
            target_generation: 9,
        },
    )]);

    assert_eq!(
        resolve_live_broker_toplevel_action(4, &grants, &descriptors, action(11, 4, 5, 9)),
        Some(surface)
    );
    for stale in [
        action(12, 4, 5, 9),
        action(11, 3, 5, 9),
        action(11, 4, 4, 9),
        action(11, 4, 5, 8),
    ] {
        assert_eq!(
            resolve_live_broker_toplevel_action(4, &grants, &descriptors, stale),
            None
        );
    }
}

#[test]
fn descriptor_generation_change_revokes_an_old_presented_action() {
    let surface = SurfaceId::new(41, 2);
    let mut descriptors = sophia_engine::ChromeDescriptorTable::default();
    descriptors.upsert(sophia_protocol::ChromeDescriptor {
        surface,
        label: None,
        icon: None,
        trust_level: sophia_protocol::TrustLevel::Unknown,
        attention: sophia_protocol::AttentionState::Notice,
        generation: 10,
    });
    let grants = BTreeMap::from([(
        surface,
        sophia_protocol::BrokerToplevelActionGrant {
            token: 11,
            revocation_epoch: 5,
            target_generation: 10,
        },
    )]);

    assert_eq!(
        resolve_live_broker_toplevel_action(4, &grants, &descriptors, action(11, 4, 5, 9)),
        None
    );
}

#[test]
fn switcher_admits_only_presented_policy_managed_surfaces() {
    let managed = SurfaceId::new(41, 2);
    let popup = SurfaceId::new(42, 2);
    let hidden = SurfaceId::new(43, 2);
    let layer = |surface| LayerSnapshot {
        input_region: None,
        translation: None,
        output: None,
        surface,
        authority_local_id: None,
        namespace: None,
        stack_rank: 0,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 60,
        },
        source: BufferSource::None,
        source_size: Size {
            width: 80,
            height: 60,
        },
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
    };
    let layers = BTreeMap::from([(managed, layer(managed)), (popup, layer(popup))]);
    let roles = BTreeMap::from([
        (
            managed,
            sophia_protocol::SurfacePresentationRole::PolicyManaged,
        ),
        (
            popup,
            sophia_protocol::SurfacePresentationRole::ClientPositioned,
        ),
        (
            hidden,
            sophia_protocol::SurfacePresentationRole::PolicyManaged,
        ),
    ]);

    assert_eq!(
        live_shell_activation_surfaces(&layers, &roles),
        BTreeSet::from([managed])
    );
}

#[test]
fn descriptor_reservations_cannot_exceed_or_invent_the_profile_allowance() {
    let claim = |thickness_px| {
        Some(sophia_protocol::ShellV1WorkAreaReservation {
            edge: sophia_protocol::ShellV1ReservationEdge::Bottom,
            thickness_px,
        })
    };

    assert!(reservation_within_profile(None, None));
    assert!(reservation_within_profile(None, Some(32)));
    assert!(!reservation_within_profile(claim(1), None));
    assert!(reservation_within_profile(claim(32), Some(32)));
    assert!(!reservation_within_profile(claim(33), Some(32)));
}

mod indicator_activation {
    use crate::live_session::metadata_shell::indicators::classify_indicator_activation;
    use sophia_protocol::{
        OutputId, ShellIndicator, ShellIndicatorActivation,
        ShellIndicatorActivationStatus as Status, ShellIndicatorSnapshot,
    };

    fn published() -> ShellIndicatorSnapshot {
        ShellIndicatorSnapshot {
            connection_epoch: 5,
            generation: 6,
            active_output: Some(OutputId::from_raw(2)),
            statuses: Vec::new(),
            indicators: vec![
                ShellIndicator {
                    output: OutputId::from_raw(1),
                    indicator: 11,
                    action: 41,
                    slot: 0,
                    state_bits: 0,
                    label: "web".to_owned(),
                },
                ShellIndicator {
                    output: OutputId::from_raw(1),
                    indicator: 12,
                    action: 0,
                    slot: 1,
                    state_bits: 0,
                    label: "code".to_owned(),
                },
            ],
        }
    }

    fn activation(indicator: u64, action: u64) -> ShellIndicatorActivation {
        ShellIndicatorActivation {
            connection_epoch: 5,
            snapshot_generation: 6,
            output: OutputId::from_raw(1),
            indicator,
            action,
            event_id: 77,
        }
    }

    #[test]
    fn a_published_pill_is_accepted() {
        assert_eq!(
            classify_indicator_activation(Some(&published()), &activation(11, 41)),
            Status::Accepted
        );
    }

    #[test]
    fn an_activation_against_a_replaced_set_is_stale() {
        let mut later = published();
        later.generation = 7;
        assert_eq!(
            classify_indicator_activation(Some(&later), &activation(11, 41)),
            Status::Stale
        );
    }

    #[test]
    fn an_activation_before_anything_was_published_is_stale() {
        assert_eq!(
            classify_indicator_activation(None, &activation(11, 41)),
            Status::Stale
        );
    }

    #[test]
    fn a_new_connection_epoch_makes_an_activation_stale() {
        let mut reconnected = published();
        reconnected.connection_epoch = 6;
        assert_eq!(
            classify_indicator_activation(Some(&reconnected), &activation(11, 41)),
            Status::Stale
        );
    }

    /// The shell cannot mint an action it was never shown.
    #[test]
    fn an_invented_action_is_unknown() {
        assert_eq!(
            classify_indicator_activation(Some(&published()), &activation(11, 999)),
            Status::Unknown
        );
    }

    /// Nor borrow a real action from a different output.
    #[test]
    fn an_action_from_another_output_is_unknown() {
        let mut foreign = activation(11, 41);
        foreign.output = OutputId::from_raw(2);
        assert_eq!(
            classify_indicator_activation(Some(&published()), &foreign),
            Status::Unknown
        );
    }

    /// Nor pair a real action with a different pill.
    #[test]
    fn a_mismatched_indicator_and_action_pair_is_unknown() {
        assert_eq!(
            classify_indicator_activation(Some(&published()), &activation(12, 41)),
            Status::Unknown
        );
    }

    /// A pill published with no action is not activatable, and zero must not be
    /// honoured as though it were one.
    #[test]
    fn a_pill_without_an_action_is_unauthorized() {
        assert_eq!(
            classify_indicator_activation(Some(&published()), &activation(12, 0)),
            Status::Unauthorized
        );
    }
}

mod indicator_projection {
    use crate::live_session::metadata_shell::indicators::indicator_snapshot;
    use sophia_protocol::{OutputId, PolicyProjectionIndicator, PolicyProjectionOutputStatus};

    fn publication(
        indicators: Vec<PolicyProjectionIndicator>,
    ) -> sophia_engine::PolicyIndicatorPublication {
        sophia_engine::PolicyIndicatorPublication {
            tab_groups: Vec::new(),
            generation: 6,
            connection_epoch: Some(5),
            indicators,
            output_statuses: vec![PolicyProjectionOutputStatus {
                output: OutputId::from_raw(2),
                focus_bits: 1,
                layout: "Scroller".to_owned(),
            }],
        }
    }

    fn indicator(action: Option<u64>) -> PolicyProjectionIndicator {
        PolicyProjectionIndicator {
            output: OutputId::from_raw(1),
            slot: 0,
            indicator: 11,
            action: action.map(sophia_protocol::WmActionId::from_raw),
            state_bits: 1,
            label: "web".to_owned(),
        }
    }

    /// An indicator with no action must publish zero. Identities allocate from
    /// one, so zero cannot collide with a real action, and the shell answers an
    /// activation naming it as unauthorized rather than honouring it.
    #[test]
    fn an_absent_action_publishes_the_zero_sentinel() {
        let snapshot = indicator_snapshot(&publication(vec![indicator(None)]), None, 5);
        assert_eq!(snapshot.indicators[0].action, 0);
    }

    #[test]
    fn a_present_action_is_carried_unchanged() {
        let snapshot = indicator_snapshot(&publication(vec![indicator(Some(41))]), None, 5);
        assert_eq!(snapshot.indicators[0].action, 41);
    }

    /// The case the vocabulary exists for: an output is focused while holding no
    /// indicator, so only the separate global identity can say where focus is.
    #[test]
    fn an_active_output_survives_with_no_indicators() {
        let snapshot = indicator_snapshot(&publication(Vec::new()), Some(OutputId::from_raw(2)), 5);
        assert_eq!(snapshot.active_output, Some(OutputId::from_raw(2)));
        assert!(snapshot.indicators.is_empty());
        assert_eq!(snapshot.statuses.len(), 1);
    }

    #[test]
    fn an_absent_active_output_stays_absent() {
        let snapshot = indicator_snapshot(&publication(vec![indicator(Some(41))]), None, 5);
        assert_eq!(snapshot.active_output, None);
    }

    /// Generation comes from the publication and the epoch from the connection;
    /// confusing them would let a reconnect look like a fresh set, or a new set
    /// look like a stale one.
    #[test]
    fn generation_and_epoch_come_from_their_own_sources() {
        let snapshot = indicator_snapshot(&publication(vec![indicator(Some(41))]), None, 9);
        assert_eq!(snapshot.generation, 6);
        assert_eq!(snapshot.connection_epoch, 9);
    }

    #[test]
    fn output_status_fields_are_carried_verbatim() {
        let snapshot = indicator_snapshot(&publication(Vec::new()), None, 5);
        let status = &snapshot.statuses[0];
        assert_eq!(status.output, OutputId::from_raw(2));
        assert_eq!(status.focus_bits, 1);
        assert_eq!(status.layout, "Scroller");
    }
}
