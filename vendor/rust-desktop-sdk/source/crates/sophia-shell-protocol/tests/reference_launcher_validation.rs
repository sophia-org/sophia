use sophia_shell_protocol::*;

fn shortcut() -> ShellShortcut {
    ShellShortcut {
        slot: 1,
        chord: "Mod+1".into(),
        action: "workspace".into(),
        label: None,
        group: None,
    }
}
fn reference() -> ShellReferenceCandidate {
    ShellReferenceCandidate {
        connection_epoch: 1,
        catalog_generation: 2,
        request_generation: 3,
        candidate_generation: 4,
        output: OutputId::from_raw(5),
        visible: true,
        page: u16::MAX,
        style: ShellReferenceStyle {
            body_size: 32,
            title_size: 48,
            padding: 64,
            row_gap: 32,
            key_gap: 64,
            column_gap: 64,
            border: 16,
            margin: 128,
            columns: 4,
            colors: [
                0x20ffffff, 0xff010203, 0xff010203, 0xff010203, 0xff010203, 0xff010203,
            ],
            title: "Keys".into(),
        },
        entries: vec![ShellReferenceEntry {
            slot: 1,
            key: "Mod+1".into(),
            label: "Workspace".into(),
        }],
    }
}
fn launcher() -> ShellLauncherCandidate {
    ShellLauncherCandidate {
        connection_epoch: 1,
        catalog_generation: 2,
        request_generation: 3,
        candidate_generation: 4,
        output: OutputId::from_raw(5),
        visible: true,
        selected: 4096,
        entries: vec![4096],
        font_size: 32,
        colors: [0x20ffffff, 0xff010203, 0xff010203, 0xff010203],
    }
}
fn activation() -> ShellLauncherActivation {
    ShellLauncherActivation {
        connection_epoch: 1,
        catalog_generation: 2,
        request_generation: 3,
        candidate_generation: 4,
        presentation_epoch: 5,
        activation: 6,
        slot: 4096,
    }
}

#[test]
fn shortcut_optional_values_preserve_absence_and_reject_empty_presence() {
    let base = ShellShortcutCatalog {
        connection_epoch: 1,
        generation: 2,
        entries: vec![shortcut()],
    };
    assert!(validate_shell_shortcut_catalog(&base).is_ok());
    for is_label in [true, false] {
        let max = if is_label { 128 } else { 64 };
        for text in ["x".repeat(max), "λ".repeat(max / 2)] {
            let mut value = base.clone();
            if is_label {
                value.entries[0].label = Some(text);
            } else {
                value.entries[0].group = Some(text);
            }
            assert!(validate_shell_shortcut_catalog(&value).is_ok());
        }
        for text in [
            String::new(),
            "x".repeat(max + 1),
            "a\u{202e}b".into(),
            "a\u{2066}b".into(),
            "a\nb".into(),
        ] {
            let mut value = base.clone();
            if is_label {
                value.entries[0].label = Some(text);
            } else {
                value.entries[0].group = Some(text);
            }
            assert!(validate_shell_shortcut_catalog(&value).is_err());
        }
    }
    let changes: &[fn(&mut ShellShortcutCatalog)] = &[
        |s| s.connection_epoch = 0,
        |s| s.generation = 0,
        |s| s.entries[0].slot = 0,
        |s| s.entries[0].chord.clear(),
        |s| s.entries[0].action.clear(),
        |s| s.entries[0].chord = "x".repeat(65),
        |s| s.entries[0].action = "x".repeat(129),
        |s| s.entries.push(s.entries[0].clone()),
    ];
    for change in changes {
        let mut bad = base.clone();
        change(&mut bad);
        assert!(validate_shell_shortcut_catalog(&bad).is_err());
    }
    let mut value = base.clone();
    value.entries = (1..=256)
        .map(|slot| ShellShortcut { slot, ..shortcut() })
        .collect();
    assert!(validate_shell_shortcut_catalog(&value).is_ok());
    value.entries.push(ShellShortcut {
        slot: 257,
        ..shortcut()
    });
    assert!(validate_shell_shortcut_catalog(&value).is_err());
}

#[test]
fn reference_style_bounds_do_not_invent_a_page_limit() {
    let base = reference();
    assert!(validate_shell_reference_candidate(&base).is_ok());
    type Change = fn(&mut ShellReferenceCandidate);
    let changes: &[Change] = &[
        |s| s.connection_epoch = 0,
        |s| s.catalog_generation = 0,
        |s| s.request_generation = 0,
        |s| s.candidate_generation = 0,
        |s| s.output = OutputId::INVALID,
        |s| s.style.body_size = 7,
        |s| s.style.body_size = 33,
        |s| s.style.title_size = 7,
        |s| s.style.title_size = 49,
        |s| s.style.padding = 65,
        |s| s.style.row_gap = 33,
        |s| s.style.key_gap = 65,
        |s| s.style.column_gap = 65,
        |s| s.style.border = 17,
        |s| s.style.margin = 129,
        |s| s.style.columns = 0,
        |s| s.style.columns = 5,
        |s| s.style.title.clear(),
        |s| s.style.title = "x".repeat(129),
        |s| s.style.title = "a\u{2069}b".into(),
        |s| s.entries[0].slot = 0,
        |s| s.entries[0].key.clear(),
        |s| s.entries[0].label.clear(),
        |s| s.entries[0].key = "x".repeat(65),
        |s| s.entries[0].label = "x".repeat(129),
        |s| s.entries.push(s.entries[0].clone()),
    ];
    for (i, change) in changes.iter().enumerate() {
        let mut bad = base.clone();
        change(&mut bad);
        assert!(
            validate_shell_reference_candidate(&bad).is_err(),
            "mutation {i}"
        );
    }
    for i in 1..6 {
        let mut bad = base.clone();
        bad.style.colors[i] = 0x7f010203;
        assert!(validate_shell_reference_candidate(&bad).is_err());
    }
    let mut value = base;
    value.style.body_size = 8;
    value.style.title_size = 8;
    value.style.columns = 1;
    value.entries = (1..=256)
        .map(|slot| ShellReferenceEntry {
            slot,
            key: "K".into(),
            label: "L".into(),
        })
        .collect();
    assert!(validate_shell_reference_candidate(&value).is_ok());
    value.entries.push(ShellReferenceEntry {
        slot: 257,
        key: "K".into(),
        label: "L".into(),
    });
    assert!(validate_shell_reference_candidate(&value).is_err());
}

#[test]
fn launcher_keeps_its_own_visibility_and_selection_rules() {
    let mut value = launcher();
    assert!(validate_shell_launcher_candidate(&value).is_ok());
    value.visible = false;
    assert!(validate_shell_launcher_candidate(&value).is_ok());
    value.selected = 0;
    assert!(validate_shell_launcher_candidate(&value).is_ok());
    value.entries.clear();
    value.visible = true;
    assert!(validate_shell_launcher_candidate(&value).is_ok());
    let changes: &[fn(&mut ShellLauncherCandidate)] = &[
        |s| s.connection_epoch = 0,
        |s| s.catalog_generation = 0,
        |s| s.request_generation = 0,
        |s| s.candidate_generation = 0,
        |s| s.output = OutputId::INVALID,
        |s| s.font_size = 9,
        |s| s.font_size = 33,
        |s| s.entries[0] = 0,
        |s| s.entries[0] = 4097,
        |s| s.selected = 1,
        |s| s.entries.push(4096),
    ];
    for change in changes {
        let mut bad = launcher();
        change(&mut bad);
        assert!(validate_shell_launcher_candidate(&bad).is_err());
    }
    for i in 1..4 {
        let mut bad = launcher();
        bad.colors[i] = 0x7f010203;
        assert!(validate_shell_launcher_candidate(&bad).is_err());
    }
    value = launcher();
    value.entries = (1..=32).collect();
    value.selected = 32;
    value.font_size = 10;
    assert!(validate_shell_launcher_candidate(&value).is_ok());
    value.entries.push(33);
    assert!(validate_shell_launcher_candidate(&value).is_err());
}

#[test]
fn request_text_and_epochs_are_checked_before_handoff() {
    let base = ShellLauncherRequest {
        connection_epoch: 1,
        catalog_generation: 2,
        request_generation: 3,
        output: OutputId::from_raw(4),
        output_generation: 5,
        presentation_epoch: 0,
        operation: ShellLauncherOperation::Open,
        query: String::new(),
    };
    assert!(validate_shell_launcher_request(&base).is_ok());
    let changes: &[fn(&mut ShellLauncherRequest)] = &[
        |s| s.connection_epoch = 0,
        |s| s.catalog_generation = 0,
        |s| s.request_generation = 0,
        |s| s.output = OutputId::INVALID,
        |s| s.output_generation = 0,
        |s| s.query = "x".repeat(257),
        |s| s.query = "a\u{202e}b".into(),
        |s| s.query = "\n".into(),
    ];
    for change in changes {
        let mut bad = base.clone();
        change(&mut bad);
        assert!(validate_shell_launcher_request(&bad).is_err());
    }
    let mut boundary = base.clone();
    boundary.query = "λ".repeat(128);
    assert!(validate_shell_launcher_request(&boundary).is_ok());
    let reference = ShellReferenceRequest {
        connection_epoch: 1,
        catalog_generation: 2,
        request_generation: 3,
        output: OutputId::from_raw(4),
        output_generation: 5,
        presentation_epoch: 0,
        operation: ShellReferenceOperation::Startup,
    };
    assert!(validate_shell_reference_request(reference).is_ok());
    let changes: &[fn(&mut ShellReferenceRequest)] = &[
        |s| s.connection_epoch = 0,
        |s| s.catalog_generation = 0,
        |s| s.request_generation = 0,
        |s| s.output = OutputId::INVALID,
        |s| s.output_generation = 0,
    ];
    for change in changes {
        let mut bad = reference;
        change(&mut bad);
        assert!(validate_shell_reference_request(bad).is_err());
    }
}

#[test]
fn reference_and_launcher_outcomes_preserve_nonpresented_epoch_behavior() {
    for kind in [
        ShellV1CandidateOutcomeKind::Prepared,
        ShellV1CandidateOutcomeKind::Presented,
        ShellV1CandidateOutcomeKind::Rejected,
        ShellV1CandidateOutcomeKind::Superseded,
    ] {
        for epoch in [0, 7] {
            let valid = kind != ShellV1CandidateOutcomeKind::Presented || epoch != 0;
            let reference = ShellReferenceOutcome {
                connection_epoch: 1,
                catalog_generation: 2,
                request_generation: 3,
                candidate_generation: 4,
                presentation_epoch: epoch,
                page: 1,
                pages: 2,
                kind,
            };
            assert_eq!(validate_shell_reference_outcome(reference).is_ok(), valid);
            assert!(
                validate_shell_reference_outcome(ShellReferenceOutcome {
                    pages: 0,
                    ..reference
                })
                .is_err()
            );
            assert!(
                validate_shell_reference_outcome(ShellReferenceOutcome {
                    page: 2,
                    ..reference
                })
                .is_err()
            );
            let launcher = ShellLauncherOutcome {
                connection_epoch: 1,
                request_generation: 3,
                candidate_generation: 4,
                presentation_epoch: epoch,
                kind,
            };
            assert_eq!(validate_shell_launcher_outcome(launcher).is_ok(), valid);
            assert!(
                validate_shell_launcher_outcome(ShellLauncherOutcome {
                    request_generation: 0,
                    ..launcher
                })
                .is_err()
            );
        }
    }
}

#[test]
fn every_launcher_activation_identity_is_required_even_for_a_refusal() {
    let base = activation();
    assert!(validate_shell_launcher_activation(base).is_ok());
    for consumed in [false, true] {
        assert!(
            validate_shell_launcher_activation_ack(ShellLauncherActivationAck {
                activation: base,
                consumed
            })
            .is_ok()
        );
    }
    let changes: &[fn(&mut ShellLauncherActivation)] = &[
        |a| a.connection_epoch = 0,
        |a| a.catalog_generation = 0,
        |a| a.request_generation = 0,
        |a| a.candidate_generation = 0,
        |a| a.presentation_epoch = 0,
        |a| a.activation = 0,
        |a| a.slot = 0,
        |a| a.slot = 4097,
    ];
    for change in changes {
        let mut bad = base;
        change(&mut bad);
        assert!(validate_shell_launcher_activation(bad).is_err());
        assert!(
            validate_shell_launcher_activation_ack(ShellLauncherActivationAck {
                activation: bad,
                consumed: false
            })
            .is_err()
        );
        for status in [
            ShellLaunchStatus::Started,
            ShellLaunchStatus::Rejected,
            ShellLaunchStatus::Failed,
        ] {
            assert!(
                validate_shell_launch_outcome(ShellLaunchOutcome {
                    activation: base,
                    status
                })
                .is_ok()
            );
            assert!(
                validate_shell_launch_outcome(ShellLaunchOutcome {
                    activation: bad,
                    status
                })
                .is_err()
            );
        }
    }
}
