const SHELL_HELP_SHORTCUT_ACTION: sophia_protocol::WmActionId =
    sophia_protocol::WmActionId::from_raw(u64::MAX - 1);
const SHELL_OVERVIEW_SHORTCUT_ACTION: sophia_protocol::WmActionId =
    sophia_protocol::WmActionId::from_raw(u64::MAX - 2);

const SHELL_SWITCHER_SHORTCUT_ACTION: sophia_protocol::WmActionId =
    sophia_protocol::WmActionId::from_raw(u64::MAX);

const fn is_shell_switcher_shortcut(action: sophia_protocol::WmActionId) -> bool {
    action.raw() == SHELL_SWITCHER_SHORTCUT_ACTION.raw()
}

fn session_shortcut_identity(
    shortcut: sophia_config::DesktopSessionShortcut,
) -> Option<(u16, &'static str)> {
    match shortcut {
        sophia_config::DesktopSessionShortcut::LaunchTerminal => Some((1, "spawn-terminal")),
        sophia_config::DesktopSessionShortcut::LaunchBrowser => Some((2, "spawn-browser")),
        sophia_config::DesktopSessionShortcut::CloseFocused => Some((3, "close-window")),
        sophia_config::DesktopSessionShortcut::Logout => Some((4, "logout")),
        sophia_config::DesktopSessionShortcut::ReloadProfile => Some((5, "reload-profile")),
        sophia_config::DesktopSessionShortcut::RestartWm => Some((6, "restart-wm")),
        sophia_config::DesktopSessionShortcut::ApplicationLauncher => Some((7, "application-launcher")),
        sophia_config::DesktopSessionShortcut::WindowSwitcher | sophia_config::DesktopSessionShortcut::ShortcutHelp => None,
    }
}

fn resolve_public_shortcuts(
    candidate: &sophia_config::DesktopShortcutCandidate,
    configuration: &sophia_protocol::PolicyConfiguration,
    policy_generation: u64,
    commands: &SessionCommandRegistry,
) -> Result<sophia_engine::WmShortcutRegistry, &'static str> {
    resolve_public_shortcuts_with_dropped_defaults(candidate, configuration, policy_generation, commands, &[])
}

fn resolve_public_shortcuts_with_dropped_defaults(
    candidate: &sophia_config::DesktopShortcutCandidate,
    configuration: &sophia_protocol::PolicyConfiguration,
    policy_generation: u64,
    commands: &SessionCommandRegistry,
    dropped: &[sophia_config::DesktopSessionShortcut],
) -> Result<sophia_engine::WmShortcutRegistry, &'static str> {
    if policy_generation != configuration.generation {
        return Err("shortcut and policy generations differ");
    }
    if configuration
        .actions
        .iter()
        .any(|action| is_reserved_session_action(action.action))
    {
        return Err("policy action collides with a reserved session shortcut");
    }
    let policy_actions = configuration
        .actions
        .iter()
        .filter(|action| action.session_operation_slot.is_none())
        .map(|action| (action.name.as_str(), action.action))
        .collect::<BTreeMap<_, _>>();
    let session_actions = configuration
        .actions
        .iter()
        .filter_map(|action| {
            action
                .session_operation_slot
                .map(|slot| ((slot, action.name.as_str()), action.action))
        })
        .collect::<BTreeMap<_, _>>();
    let mut bindings = Vec::with_capacity(candidate.bindings.len());
    for binding in &candidate.bindings {
        // These omissions were admitted and reported at startup only for the
        // compiled fallback. Keep the source candidate and its digest intact.
        if let sophia_config::DesktopShortcutTarget::Session(shortcut) = &binding.target
            && dropped.contains(shortcut)
        {
            continue;
        }
        if binding.chord.kind == sophia_config::DesktopShortcutBindingKind::Pointer {
            let valid_engine_gesture = matches!(
                &binding.target,
                sophia_config::DesktopShortcutTarget::PolicyAction(action)
                    if (action == "move" && binding.chord.trigger == "left"
                        || action == "resize" && binding.chord.trigger == "right")
                        && binding.chord.modifiers.bits()
                            == sophia_config::DesktopShortcutModifiers::SUPER.bits()
            );
            if !valid_engine_gesture {
                return Err("unsupported pointer shortcut");
            }
            continue;
        }
        let action = match &binding.target {
            sophia_config::DesktopShortcutTarget::LaunchApplication(name) => commands.action(name)
                .ok_or("shortcut names an unresolved application command")?,
            sophia_config::DesktopShortcutTarget::PolicyAction(name) => policy_actions
                .get(name.as_str())
                .copied()
                .ok_or("shortcut names an unregistered policy action")?,
            sophia_config::DesktopShortcutTarget::Session(
                sophia_config::DesktopSessionShortcut::WindowSwitcher,
            ) => SHELL_SWITCHER_SHORTCUT_ACTION,
            sophia_config::DesktopShortcutTarget::Session(
                sophia_config::DesktopSessionShortcut::ShortcutHelp,
            ) => SHELL_HELP_SHORTCUT_ACTION,
            sophia_config::DesktopShortcutTarget::Session(sophia_config::DesktopSessionShortcut::LaunchTerminal)
                if commands.roles.contains_key(&TERMINAL_APPLICATION_ID) => commands.roles[&TERMINAL_APPLICATION_ID],
            sophia_config::DesktopShortcutTarget::Session(sophia_config::DesktopSessionShortcut::LaunchBrowser)
                if commands.roles.contains_key(&BROWSER_APPLICATION_ID) => commands.roles[&BROWSER_APPLICATION_ID],
            sophia_config::DesktopShortcutTarget::Session(shortcut) => session_shortcut_identity(
                *shortcut,
            )
            .and_then(|identity| session_actions.get(&identity))
            .copied()
            .ok_or("shortcut names an unavailable session capability")?,
        };
        let keycode = sophia_config::desktop_shortcut_evdev_keycode(&binding.chord.trigger)
            .ok_or("shortcut trigger has no evdev identity")?;
        bindings.push(sophia_protocol::WmBindingRegistration {
            action,
            keycode,
            modifiers: sophia_protocol::WmModifierMask {
                bits: u32::from(binding.chord.modifiers.bits()),
            },
        });
    }
    // Built from prepared authorities, so there is no transport handshake to
    // fabricate before constructing Engine's shortcut registry.
    sophia_engine::WmShortcutRegistry::new(
        &bindings,
        sophia_protocol::WmCapabilities::all_supported(),
        configuration.generation,
        configuration.chrome,
    )
    .map_err(|_| "resolved shortcut registry is invalid")
}
