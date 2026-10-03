const SHELL_HELP_SHORTCUT_ACTION: sophia_protocol::WmActionId =
    sophia_protocol::WmActionId::from_raw(u64::MAX - 1);

const SHELL_SWITCHER_SHORTCUT_ACTION: sophia_protocol::WmActionId =
    sophia_protocol::WmActionId::from_raw(u64::MAX);

/// The session lock, handled by Session itself: the WM neither sees nor
/// can delay it.
const SESSION_LOCK_SHORTCUT_ACTION: sophia_protocol::WmActionId =
    sophia_protocol::WmActionId::from_raw(u64::MAX - 2);

const fn is_shell_switcher_shortcut(action: sophia_protocol::WmActionId) -> bool {
    action.raw() == SHELL_SWITCHER_SHORTCUT_ACTION.raw()
}

/// Where an ordinary activated action goes, however it was fired: a key, or
/// a deadline. Help and the switcher sit in the reserved session range but
/// are shell requests, never command launches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PhysicalActionRoute {
    SessionCommand,
    Help,
    Switcher,
    Lock,
    Policy,
}

const fn physical_action_route(action: sophia_protocol::WmActionId) -> PhysicalActionRoute {
    if action.raw() == SHELL_HELP_SHORTCUT_ACTION.raw() {
        PhysicalActionRoute::Help
    } else if is_shell_switcher_shortcut(action) {
        PhysicalActionRoute::Switcher
    } else if action.raw() == SESSION_LOCK_SHORTCUT_ACTION.raw() {
        PhysicalActionRoute::Lock
    } else if is_reserved_session_action(action) {
        PhysicalActionRoute::SessionCommand
    } else {
        PhysicalActionRoute::Policy
    }
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
        sophia_config::DesktopSessionShortcut::WindowSwitcher
        | sophia_config::DesktopSessionShortcut::ShortcutHelp
        | sophia_config::DesktopSessionShortcut::Lock => None,
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
    let plan = resolve_shortcut_plan(candidate, &policy_actions, &session_actions, commands, dropped)?;
    // Built from prepared authorities, so there is no transport handshake to
    // fabricate before constructing Engine's shortcut registry.
    sophia_engine::WmShortcutRegistry::from_plan(
        &plan,
        sophia_protocol::WmCapabilities::all_supported(),
        configuration.generation,
        configuration.chrome,
    )
    .map_err(|_| "resolved shortcut registry is invalid")
}

/// Every key shape of the profile, resolved against the policy catalog and
/// the session's commands: immediate chords, hold variants, lone modifier
/// taps, key sequences and their leaders, and the timing. Pointer bindings
/// name the engine's fixed gestures and only validate.
fn resolve_shortcut_plan(
    candidate: &sophia_config::DesktopShortcutCandidate,
    policy_actions: &BTreeMap<&str, sophia_protocol::WmActionId>,
    session_actions: &BTreeMap<(u16, &str), sophia_protocol::WmActionId>,
    commands: &SessionCommandRegistry,
    dropped: &[sophia_config::DesktopSessionShortcut],
) -> Result<sophia_engine::WmShortcutPlan, &'static str> {
    let step = |chord: &sophia_config::DesktopShortcutChord| {
        sophia_config::desktop_shortcut_evdev_keycode(&chord.trigger)
            .map(|keycode| sophia_engine::WmKeyStep {
                keycode,
                modifiers: u32::from(chord.modifiers.bits()),
            })
            .ok_or("shortcut trigger has no evdev identity")
    };
    let mut plan = sophia_engine::WmShortcutPlan {
        timing: sophia_engine::WmShortcutTiming {
            tap_ms: candidate.timing.tap_ms,
            sequence_ms: candidate.timing.sequence_ms,
        },
        ..sophia_engine::WmShortcutPlan::default()
    };
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
            sophia_config::DesktopShortcutTarget::Session(
                sophia_config::DesktopSessionShortcut::Lock,
            ) => SESSION_LOCK_SHORTCUT_ACTION,
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
        if let Some(modifier) = binding.modifier_tap() {
            plan.taps.push(sophia_engine::WmModifierTapBinding {
                modifier: u32::from(modifier.bits()),
                action,
            });
        } else if let Some(hold_ms) = binding.hold_ms {
            plan.holds.push(sophia_engine::WmHoldBinding {
                step: step(&binding.chord)?,
                hold_ms,
                action,
            });
        } else if binding.steps.is_empty() {
            let step = step(&binding.chord)?;
            plan.immediate.push(sophia_protocol::WmBindingRegistration {
                action,
                keycode: step.keycode,
                modifiers: sophia_protocol::WmModifierMask {
                    bits: step.modifiers,
                },
            });
        } else {
            plan.sequences.push(sophia_engine::WmSequenceBinding {
                steps: binding.path().map(step).collect::<Result<_, _>>()?,
                action,
            });
        }
    }
    for leader in &candidate.leaders {
        // A leader is a policy action the WM may follow, never a session
        // capability.
        let action = policy_actions
            .get(leader.action.as_str())
            .copied()
            .ok_or("shortcut leader names an unregistered policy action")?;
        plan.leaders.push(sophia_engine::WmSequenceLeader {
            steps: leader.path().map(step).collect::<Result<_, _>>()?,
            action,
        });
    }
    Ok(plan)
}
