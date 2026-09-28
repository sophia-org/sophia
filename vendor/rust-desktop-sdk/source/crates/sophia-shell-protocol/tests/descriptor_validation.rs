use sophia_shell_protocol::*;

fn descriptor(slot: u16) -> ShellV1Descriptor {
    ShellV1Descriptor {
        slot,
        generation: 4,
        label: Some(DisplayLabel {
            text: "window".into(),
            redacted: false,
        }),
        trust_level: TrustLevel::Trusted,
        attention: AttentionState::None,
        action: ToplevelActionCapabilityRef {
            token: 6,
            issuer_epoch: 7,
            issuer_revocation_epoch: 8,
            recipient_epoch: 9,
            target_slot: slot,
            target_generation: 4,
        },
    }
}
fn snapshot(count: u16) -> ShellV1DescriptorSnapshot {
    ShellV1DescriptorSnapshot {
        connection_epoch: 9,
        snapshot_generation: 2,
        output: OutputId::from_raw(3),
        output_generation: 4,
        broker_epoch: 7,
        broker_revocation_epoch: 8,
        descriptors: (1..=count).map(descriptor).collect(),
    }
}
fn candidate() -> ShellV1Candidate {
    ShellV1Candidate {
        connection_epoch: 9,
        snapshot_generation: 2,
        candidate_generation: 3,
        output: OutputId::from_raw(4),
        visible: true,
        selected_slot: Some(1),
        reservation: Some(ShellV1WorkAreaReservation {
            edge: ShellV1ReservationEdge::Top,
            thickness_px: 512,
        }),
        entries: vec![ShellV1CandidateEntry {
            slot: 1,
            generation: 4,
        }],
    }
}
fn tab_snapshot() -> ShellTabSnapshot {
    ShellTabSnapshot {
        connection_epoch: 9,
        generation: 2,
        groups: vec![ShellTabGroup {
            slot: 1,
            output: OutputId::from_raw(3),
            focused: true,
            selected_slot: Some(1),
            entries: (1..=2048).map(descriptor).collect(),
        }],
    }
}

#[test]
fn snapshot_bounds_and_action_binding() {
    assert!(validate_shell_descriptor_snapshot(&snapshot(0)).is_ok());
    assert!(validate_shell_descriptor_snapshot(&snapshot(16)).is_ok());
    assert!(validate_shell_descriptor_snapshot(&snapshot(17)).is_err());
    let changes: &[fn(&mut ShellV1DescriptorSnapshot)] = &[
        |s| s.connection_epoch = 0,
        |s| s.snapshot_generation = 0,
        |s| s.output = OutputId::INVALID,
        |s| s.output_generation = 0,
        |s| s.broker_epoch = 0,
        |s| s.broker_revocation_epoch = 0,
        |s| s.descriptors[0].slot = 0,
        |s| s.descriptors[0].generation = 0,
        |s| s.descriptors[0].action.token = 0,
        |s| s.descriptors[0].action.issuer_epoch = 17,
        |s| s.descriptors[0].action.issuer_revocation_epoch = 18,
        |s| s.descriptors[0].action.recipient_epoch = 19,
        |s| s.descriptors[0].action.target_slot = 2,
        |s| s.descriptors[0].action.target_generation = 14,
        |s| s.descriptors.push(s.descriptors[0].clone()),
    ];
    for (i, change) in changes.iter().enumerate() {
        let mut value = snapshot(1);
        change(&mut value);
        assert!(
            validate_shell_descriptor_snapshot(&value).is_err(),
            "mutation {i}"
        );
    }
}

#[test]
fn descriptor_label_rule_is_not_the_launcher_text_rule() {
    let mut value = descriptor(1);
    for text in [
        "x".repeat(128),
        "λ".repeat(64),
        "left\u{202e}right".into(),
        "\u{2066}x\u{2069}".into(),
    ] {
        value.label.as_mut().unwrap().text = text;
        assert!(validate_shell_descriptor(&value, 9).is_ok());
    }
    for text in [
        "".into(),
        "x".repeat(129),
        "λ".repeat(65),
        "a\nb".into(),
        "a\0b".into(),
    ] {
        value.label.as_mut().unwrap().text = text;
        assert!(validate_shell_descriptor(&value, 9).is_err());
    }
    value.label = None;
    assert!(validate_shell_descriptor(&value, 9).is_ok());
    assert!(validate_shell_descriptor(&value, 0).is_err());
}

#[test]
fn action_requires_every_nonzero_identity() {
    let changes: &[fn(&mut ToplevelActionCapabilityRef)] = &[
        |a| a.token = 0,
        |a| a.issuer_epoch = 0,
        |a| a.issuer_revocation_epoch = 0,
        |a| a.recipient_epoch = 0,
        |a| a.target_slot = 0,
        |a| a.target_generation = 0,
    ];
    for change in changes {
        let mut value = descriptor(1).action;
        change(&mut value);
        assert!(validate_toplevel_action(value).is_err());
    }
}

#[test]
fn visible_selection_and_reservation_are_coherent() {
    assert!(validate_shell_descriptor_candidate(&candidate()).is_ok());
    let changes: &[fn(&mut ShellV1Candidate)] = &[
        |s| s.connection_epoch = 0,
        |s| s.snapshot_generation = 0,
        |s| s.candidate_generation = 0,
        |s| s.output = OutputId::INVALID,
        |s| s.visible = false,
        |s| s.entries.clear(),
        |s| s.selected_slot = None,
        |s| s.selected_slot = Some(2),
        |s| s.entries[0].generation = 0,
        |s| s.entries[0].slot = 0,
        |s| s.entries.push(s.entries[0]),
        |s| s.reservation.as_mut().unwrap().thickness_px = 0,
        |s| s.reservation.as_mut().unwrap().thickness_px = 513,
    ];
    for change in changes {
        let mut value = candidate();
        change(&mut value);
        assert!(validate_shell_descriptor_candidate(&value).is_err());
    }
    let mut value = candidate();
    value.visible = false;
    value.entries.clear();
    value.selected_slot = None;
    assert!(validate_shell_descriptor_candidate(&value).is_err());
    value.reservation = None;
    assert!(validate_shell_descriptor_candidate(&value).is_ok());
    value = candidate();
    value.entries = (1..=16)
        .map(|slot| ShellV1CandidateEntry {
            slot,
            generation: 4,
        })
        .collect();
    assert!(validate_shell_descriptor_candidate(&value).is_ok());
    value.entries.push(ShellV1CandidateEntry {
        slot: 17,
        generation: 4,
    });
    assert!(validate_shell_descriptor_candidate(&value).is_err());
}

#[test]
fn tabs_allow_full_cardinality_and_distinct_issuers_without_a_sixteen_row_limit() {
    let mut value = tab_snapshot();
    value.groups[0].entries[1].action.issuer_epoch = 27;
    value.groups[0].entries[1].action.issuer_revocation_epoch = 28;
    assert!(validate_shell_tab_snapshot(&value).is_ok());
    value.groups[0].entries.push(descriptor(2049));
    assert!(validate_shell_tab_snapshot(&value).is_err());
    let mut value = tab_snapshot();
    value.groups[0].entries.clear();
    value.groups[0].selected_slot = None;
    value.groups = (1..=1024)
        .map(|slot| ShellTabGroup {
            slot,
            ..value.groups[0].clone()
        })
        .collect();
    assert!(validate_shell_tab_snapshot(&value).is_ok());
    value.groups.push(ShellTabGroup {
        slot: 1025,
        ..value.groups[0].clone()
    });
    assert!(validate_shell_tab_snapshot(&value).is_err());
}

#[test]
fn tab_slots_are_global_but_selection_is_group_local() {
    let mut value = tab_snapshot();
    value.groups[0].entries = vec![descriptor(1)];
    value.groups.push(ShellTabGroup {
        slot: 2,
        output: OutputId::from_raw(3),
        focused: false,
        selected_slot: Some(2),
        entries: vec![descriptor(2)],
    });
    assert!(validate_shell_tab_snapshot(&value).is_ok());
    let changes: &[fn(&mut ShellTabSnapshot)] = &[
        |s| s.connection_epoch = 0,
        |s| s.generation = 0,
        |s| s.groups[1].slot = 1,
        |s| s.groups[1].slot = 0,
        |s| s.groups[1].output = OutputId::INVALID,
        |s| s.groups[1].selected_slot = Some(1),
        |s| s.groups[1].selected_slot = None,
        |s| s.groups[1].entries[0] = descriptor(1),
        |s| s.groups[1].entries[0].action.recipient_epoch = 10,
    ];
    for change in changes {
        let mut bad = value.clone();
        change(&mut bad);
        assert!(validate_shell_tab_snapshot(&bad).is_err());
    }
}

#[test]
fn tab_candidate_preserves_owner_checks_for_order_and_high_bit() {
    let mut value = ShellTabCandidate {
        connection_epoch: 9,
        snapshot_generation: 2,
        candidate_generation: 1 << 63,
        groups: vec![2, 1],
    };
    assert!(validate_shell_tab_candidate(&value).is_ok());
    value.groups = vec![2, 2];
    assert!(validate_shell_tab_candidate(&value).is_err());
    value.groups = vec![0];
    assert!(validate_shell_tab_candidate(&value).is_err());
    value.groups = (1..=1024).collect();
    assert!(validate_shell_tab_candidate(&value).is_ok());
    value.groups.push(1025);
    assert!(validate_shell_tab_candidate(&value).is_err());
}

#[test]
fn descriptor_outcomes_and_activation_require_exact_identity() {
    for kind in [
        ShellV1CandidateOutcomeKind::Prepared,
        ShellV1CandidateOutcomeKind::Presented,
        ShellV1CandidateOutcomeKind::Rejected,
        ShellV1CandidateOutcomeKind::Superseded,
    ] {
        for epoch in [0, 7] {
            let value = ShellV1CandidateOutcome {
                connection_epoch: 9,
                candidate_generation: 2,
                presentation_epoch: epoch,
                kind,
            };
            assert_eq!(
                validate_shell_descriptor_outcome(value).is_ok(),
                (kind == ShellV1CandidateOutcomeKind::Presented) == (epoch != 0)
            );
        }
    }
    let value = ShellV1Activation {
        connection_epoch: 9,
        candidate_generation: 2,
        presentation_epoch: 3,
        activation: 4,
        action: descriptor(1).action,
    };
    assert!(validate_shell_descriptor_activation(value).is_ok());
    let changes: &[fn(&mut ShellV1Activation)] = &[
        |a| a.connection_epoch = 0,
        |a| a.candidate_generation = 0,
        |a| a.presentation_epoch = 0,
        |a| a.activation = 0,
        |a| a.action.recipient_epoch = 10,
    ];
    for change in changes {
        let mut bad = value;
        change(&mut bad);
        assert!(validate_shell_descriptor_activation(bad).is_err());
    }
    for disposition in [
        ShellV1ActivationDisposition::Consumed,
        ShellV1ActivationDisposition::RejectedStale,
    ] {
        let ack = ShellV1ActivationAck {
            connection_epoch: 9,
            activation: 4,
            disposition,
        };
        assert!(validate_shell_descriptor_activation_ack(ack).is_ok());
        assert!(
            validate_shell_descriptor_activation_ack(ShellV1ActivationAck {
                activation: 0,
                ..ack
            })
            .is_err()
        );
        assert!(
            validate_shell_descriptor_activation_ack(ShellV1ActivationAck {
                connection_epoch: 0,
                ..ack
            })
            .is_err()
        );
    }
}
