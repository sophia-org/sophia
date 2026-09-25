use super::*;

fn browser_action() -> sophia_protocol::PolicyActionRegistration {
    sophia_protocol::PolicyActionRegistration {
        action: WmActionId::from_raw(2),
        name: "spawn-browser".into(),
        session_operation_slot: Some(2),
    }
}

fn browser_fixture() -> ReloadFixture {
    let source = ConfigFixture::from_documents(
        super::super::CORE,
        "schema 1\nshell { enabled #false; }\nsession { browser \"brave-origin\"; }\nshortcut { profile \"operator\"; bind \"Super+b\" \"session:spawn-browser\"; }\n",
        &[],
    );
    let mut fixture = ReloadFixture::from_config_with_actions(source, vec![browser_action()]);
    fixture.wm.command_registry =
        SessionCommandRegistry::prepare(1, &fixture.source.config.applications)
            .unwrap()
            .with_policy_launch_roles(true);
    fixture
        .wm
        .public
        .as_mut()
        .unwrap()
        .session_operations
        .retain(|operation| operation.slot != 2);
    fixture
}

#[test]
fn unavailable_bound_operation_is_rejected_even_when_the_catalog_offer_is_filtered() {
    let mut fixture = browser_fixture();
    let mut offered = fixture.configuration();
    offered.actions.push(browser_action());
    assert_eq!(
        fixture.stage_configuration(&offered),
        sophia_protocol::PolicyProjectionOutcome::RejectedInvalid,
    );
    assert!(fixture.wm.pending_policy_configuration.is_none());
}

#[test]
fn reported_default_omissions_survive_configuration_and_replacement() {
    let mut fixture = browser_fixture();
    fixture
        .wm
        .public
        .as_mut()
        .unwrap()
        .dropped_default_shortcuts = vec![sophia_config::DesktopSessionShortcut::LaunchBrowser];
    let mut offered = fixture.configuration();
    offered.actions.push(browser_action());
    for _ in 0..2 {
        assert_eq!(
            fixture.stage_configuration(&offered),
            sophia_protocol::PolicyProjectionOutcome::Committed
        );
        fixture.settle(true);
        let public = fixture.wm.public.as_ref().unwrap();
        assert!(
            public
                .accepted_configuration
                .as_ref()
                .unwrap()
                .actions
                .is_empty()
        );
        assert!(public.actions.is_empty());
        assert_eq!(fixture.wm.shortcuts.as_ref().unwrap().binding_count(), 0);
    }
}

#[test]
fn compiled_desktop_with_no_applications_accepts_the_standard_policy_vocabulary() {
    let mut source = ConfigFixture::new(&[]);
    source.config = PersistentXtermSessionConfig::from_args(&[
        "--no-config".into(),
        "--no-input".into(),
        "--session-mode=normal".into(),
        "--wm-process=/usr/bin/true".into(),
        "--wm-interface=sophia_wm_v1".into(),
    ])
    .unwrap();
    let mut names = BTreeMap::new();
    for binding in &source.config.shortcut_profile_candidate.bindings {
        if let sophia_config::DesktopShortcutTarget::PolicyAction(name) = &binding.target {
            names.insert(name.clone(), None);
        }
    }
    for shortcut in [
        sophia_config::DesktopSessionShortcut::LaunchTerminal,
        sophia_config::DesktopSessionShortcut::LaunchBrowser,
        sophia_config::DesktopSessionShortcut::CloseFocused,
        sophia_config::DesktopSessionShortcut::Logout,
        sophia_config::DesktopSessionShortcut::ReloadProfile,
        sophia_config::DesktopSessionShortcut::RestartWm,
        sophia_config::DesktopSessionShortcut::ApplicationLauncher,
    ] {
        let (slot, name) = session_shortcut_identity(shortcut).unwrap();
        names.insert(name.into(), Some(slot));
    }
    let actions = names
        .into_iter()
        .enumerate()
        .map(
            |(index, (name, slot))| sophia_protocol::PolicyActionRegistration {
                action: WmActionId::from_raw(index as u64 + 1),
                name,
                session_operation_slot: slot,
            },
        )
        .collect::<Vec<_>>();
    let mut fixture = ReloadFixture::from_config_with_actions(source, actions.clone());
    let mut offered = fixture.configuration();
    offered.actions = actions;
    assert_eq!(
        fixture.stage_configuration(&offered),
        sophia_protocol::PolicyProjectionOutcome::Committed
    );
    fixture.settle(true);
    let public = fixture.wm.public.as_ref().unwrap();
    assert!(!public.actions.is_empty());
    for action in &public.actions {
        if let Some(slot) = action.session_operation_slot {
            assert!(
                public
                    .session_operations
                    .iter()
                    .any(|operation| operation.slot == slot)
            );
        }
    }
    let mut shortcuts = fixture.wm.shortcuts.clone().unwrap();
    let seat = SeatId::from_raw(1);
    shortcuts.route_key(seat, 125, true); // Super
    assert!(shortcuts.route_key(seat, 48, true).action.is_none()); // browser
    assert!(shortcuts.route_key(seat, 28, true).action.is_none()); // terminal
    assert!(shortcuts.route_key(seat, 16, true).action.is_some()); // close

    // A core reload may supply an application that the fallback could not
    // previously launch. Recompute omissions with the new command registry.
    let report = fixture
        .wm
        .reload_core_launches(
            &mut fixture.source.config,
            b"schema 2\nsession { application \"terminal\" id=1 executable=\"/usr/bin/true\"; }\n",
        )
        .unwrap();
    assert_eq!(
        report.disposition,
        sophia_config::ReloadDisposition::Applied
    );
    assert!(
        !fixture
            .source
            .config
            .dropped_shortcuts
            .contains(&sophia_config::DesktopSessionShortcut::LaunchTerminal)
    );
    let mut shortcuts = fixture.wm.shortcuts.clone().unwrap();
    shortcuts.route_key(seat, 125, true);
    assert!(shortcuts.route_key(seat, 28, true).action.is_some());
    assert!(shortcuts.route_key(seat, 48, true).action.is_none());
}
