{
                if !control_plane_applied {
                    report.keys_observed = report.keys_observed.saturating_add(1);
                    if !report.devices_keyed.contains(&event.device) {
                        report.devices_keyed.push(event.device);
                    }
                    keyboard_coverage.observe_key_at_device(event.device, keycode, pressed);
                    let launcher_text=launcher.as_mut().map(|(capture,keyboard)|keyboard.observe(keycode,pressed,capture.active()));

                    match virtual_terminal_chord.observe_at_device(
                        event.device,
                        keycode,
                        pressed,
                        event.time_msec,
                    ) {
                    VirtualTerminalChordAction::Pass => {}
                    VirtualTerminalChordAction::Consume => continue,
                    VirtualTerminalChordAction::Activate(terminal) => {
                        report.virtual_terminal_trigger_keycode = Some(keycode);
                        report.virtual_terminal_modifier_keycodes =
                            virtual_terminal_chord
                                .pressed_modifier_keycodes_for(event.device);
                        keyboard_coverage.observe_virtual_terminal(terminal);
                        for modifier_keycode in virtual_terminal_chord
                            .pressed_modifier_keycodes_for(event.device)
                            .into_iter()
                            .flatten()
                        {
                            if let Some(shortcuts) = shortcuts.as_deref_mut() {
                                let _ = shortcuts.route_key(event.seat, modifier_keycode, false);
                            }
                            let _ = modifiers.map_evdev_key(modifier_keycode, false);
                            if let Some((_, keyboard)) = launcher.as_mut() {
                                // Releases may occur on the destination VT;
                                // release every local keyboard view before leaving.
                                let _ = keyboard.observe(modifier_keycode, false, false);
                            }
                            if routing_mode != PhysicalInputRoutingMode::Full {
                                discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                            }
                            let mut release = event.clone();
                            release.kind = sophia_protocol::InputEventKind::Key {
                                keycode: modifier_keycode,
                                pressed: false,
                            };
                            let release = match focus
                                .route_keyboard_event(release, committed_surfaces)
                            {
                                FocusedInputRoute::Routed(release) => release,
                                FocusedInputRoute::NoFocus(_)
                                | FocusedInputRoute::StaleFocus(_)
                                | FocusedInputRoute::UnsupportedEvent(_) => continue,
                            };
                            let Some(target_surface) = release.target_surface else {
                                discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                            };
                            let delivery =
                                XAuthorityInputDeliveryId::from_raw(*next_input_delivery);
                            *next_input_delivery =
                                next_input_delivery.checked_add(1).ok_or(
                                    "live-session input delivery ID exhausted",
                                )?;
                            // A release the client never sees is a modifier
                            // held down forever, so leave the key recorded as
                            // pressed and let the epoch close be the barrier.
                            if !route_bounded_input(
                                input_sender,
                                XAuthorityRoutedInput {
                                    request: sophia_protocol::RoutedInputRequest {
                                        serial: release.serial,
                                        seat: release.seat,
                                        device: release.device,
                                        time_msec: release.time_msec,
                                        target_surface,
                                        global_position: Point::default(),
                                        local_position: Point::default(),
                                        kind: release.kind,
                                    },
                                    route_lease: None,
                                    delivery: Some(delivery),
                                    mode: XAuthorityRoutedInputMode::Deliver,
                                    origin: sophia_x_authority::XAuthorityRoutedInputOrigin::Physical,
                                },
                                sophia_protocol::CapacityClass::TerminatingBoundary,
                                &mut report.ingress_saturation,
                            )? {
                                discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                            }
                            client_keys.record_routed(
                                SessionClientPressedKey {
                                    surface: target_surface,
                                    seat: release.seat,
                                    device: release.device,
                                    keycode: modifier_keycode,
                                },
                                false,
                            );
                            report.keys_routed = report.keys_routed.saturating_add(1);
                            report.key_targets.push(target_surface);
                            report.virtual_terminal_modifier_releases = report
                                .virtual_terminal_modifier_releases
                                .saturating_add(1);
                            report.deliveries.push(delivery);
                        }
                        report.virtual_terminal = Some(terminal);
                        discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                    }
                    }
                    if emergency_chord.observe_at_device(event.device, keycode, pressed)
                        == EmergencyChordAction::Triggered
                    {
                        report.emergency_exit = true;
                        discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                    }
                    let decision = if routing_mode != PhysicalInputRoutingMode::CursorOnly {
                        shortcuts.as_deref_mut().map(|router|router.route_key(event.seat,keycode,pressed))
                    } else {None};
                    let switcher=decision.as_ref().is_some_and(|d|d.action.is_some_and(is_shell_switcher_shortcut));
                    let help=decision.as_ref().is_some_and(|d|d.action==Some(SHELL_HELP_SHORTCUT_ACTION));
                    if !switcher && !help && let Some((capture,keyboard))=launcher.as_mut() {
                        let (text,clear)=launcher_text.as_ref().map_or((None,false),|(text,clear)|(text.as_deref(),*clear));
                        let(consumed,input)=capture.route(&event,text,pointer.position(),clear,keyboard.command_modifier_active());
                        if input.as_ref().is_some_and(|event| matches!(event.input, sophia_engine::LauncherInput::CaptureCapacityExceeded)) {
                            return Err("native launcher capture capacity exhausted".into());
                        }
                        report.launcher_events.extend(input);
                        if consumed {
                            if let Some(policy) = policy_presentation.as_mut() { policy.capture.revoke(); }
                            key_repeat.cancel_seat(event.seat);discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                    }
                    if !switcher && let Some(capture)=reference_capture.as_deref_mut() {
                        let (consumed,operation)=capture.route(&event);
                        report.reference_operations.extend(operation);
                        if consumed {
                            if let Some(policy) = policy_presentation.as_mut() { policy.capture.revoke(); }
                            key_repeat.cancel_seat(event.seat);discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                    }
                    if let Some(policy) = policy_presentation.as_mut() {
                        let protected = decision.as_ref().and_then(|decision| decision.action).is_some_and(|action|
                            is_reserved_session_action(action) || policy.protected_actions.contains(&action));
                        if !protected {
                            let mask = shortcuts.as_deref().map_or(sophia_protocol::WmModifierMask { bits: 0 }, |router| router.modifier_mask(event.seat));
                            let application_active = client_keys.pending_len() != 0
                                || application_route_leases.as_deref().is_some_and(|leases| leases.leases().next().is_some())
                                || pointer_focus_handoff.as_deref().and_then(PointerFocusHandoffState::target).is_some();
                            let disposition = if policy.keyboard_needs_shield(input_projections) {
                                policy.capture.block_key(event.seat, event.device, keycode, pressed, application_active)
                            } else {
                                policy.capture.key(policy.state, event.seat, event.device, keycode, pressed, mask, application_active)
                            };
                            if record_policy_input(disposition, &mut report) {
                                key_repeat.cancel_seat(event.seat);
                                continue;
                            }
                        }
                    }
                    if let Some(decision)=decision && decision.consumed {
                        if pressed && key_repeat_map.evdev_key_repeats(keycode) {key_repeat.cancel_seat(event.seat);}
                        report.wm_actions.extend(decision.action);
                        report.policy_inputs.extend(decision.action.map(PhysicalPolicyInput::Action));
                        continue;
                    }
                }
                if !control_plane_applied
                    && !matches!(
                        routing_mode,
                        PhysicalInputRoutingMode::Full | PhysicalInputRoutingMode::ControlPlaneOnly
                    )
                {
                    continue;
                }
                if crate::input_proof::pointer_proof_suppresses_return(
                    pointer_proof_required,
                    keycode,
                    physical_text_proof
                        .as_deref()
                        .is_some_and(PhysicalTextProof::is_complete),
                ) {
                    report.return_suppressed = true;
                    continue;
                }
                let event = if control_plane_applied {
                    let Some(target) = event.target_surface else {
                        continue;
                    };
                    if focus.focused_surface(event.seat) != Some(target)
                        || !committed_surfaces
                            .iter()
                            .any(|committed| committed.surface == target)
                    {
                        continue;
                    }
                    event
                } else {
                    match focus.route_keyboard_event(event, committed_surfaces) {
                        FocusedInputRoute::Routed(event) => event,
                        FocusedInputRoute::NoFocus(_) => {
                            report.keys_suppressed_no_focus =
                                report.keys_suppressed_no_focus.saturating_add(1);
                            continue;
                        }
                        FocusedInputRoute::StaleFocus(_) => {
                            report.keys_suppressed_stale_focus =
                                report.keys_suppressed_stale_focus.saturating_add(1);
                            continue;
                        }
                        FocusedInputRoute::UnsupportedEvent(_) => continue,
                    }
                };
                let Some(target_surface) = event.target_surface else {
                    continue;
                };
                if !control_plane_applied
                    && routing_mode == PhysicalInputRoutingMode::ControlPlaneOnly
                {
                    if let Some(handoff) = keyboard_focus_handoff.as_deref_mut() {
                        let changed_target = handoff
                            .target()
                            .is_some_and(|held| held != target_surface);
                        let deferred_press =
                            pressed.then_some((event.serial, event.time_msec));
                        if handoff.defer(target_surface, now_msec, event).is_err() {
                            if changed_target {
                                report.keyboard_focus_handoff_stale_drops = report
                                    .keyboard_focus_handoff_stale_drops
                                    .saturating_add(1);
                            } else {
                                report.keyboard_focus_handoff_capacity_drops = report
                                    .keyboard_focus_handoff_capacity_drops
                                    .saturating_add(1);
                            }
                        } else if let Some(deferred_press) = deferred_press {
                            report.deferred_key_presses.push(deferred_press);
                        }
                    }
                    continue;
                }
                let key = SessionClientPressedKey {
                    surface: target_surface,
                    seat: event.seat,
                    device: event.device,
                    keycode,
                };
                if !pressed && !client_keys.release_is_routable(key) {
                    client_keys.record_routed(key, false);
                    continue;
                }
                let evdev_keycode = keycode;
                if !pressed {
                    let _ = key_repeat.release(event.seat, event.device, evdev_keycode);
                }
                let Some((keycode, state)) = modifiers.map_evdev_key(keycode, pressed) else {
                    continue;
                };
                if !crate::input_proof::physical_text_proof_ignores_evdev_key(evdev_keycode)
                    && let Some(proof) = physical_text_proof.as_deref_mut()
                    && !proof.is_complete() {
                        let observed = PhysicalTextProofEvent {
                            keycode,
                            pressed,
                            state,
                        };
                        if let Err(mismatch) = proof.observe(observed) {
                            return Err(format!(
                            "physical text proof sequence mismatch at event {}: expected keycode={} pressed={} state={} observed keycode={} pressed={} state={}",
                            mismatch.event_index,
                            mismatch.expected.keycode,
                            mismatch.expected.pressed,
                            mismatch.expected.state,
                            mismatch.observed.keycode,
                            mismatch.observed.pressed,
                            mismatch.observed.state,
                        )
                        .into());
                        }
                    }
                let delivery = XAuthorityInputDeliveryId::from_raw(*next_input_delivery);
                *next_input_delivery = next_input_delivery
                    .checked_add(1)
                    .ok_or("live-session input delivery ID exhausted")?;
                if !route_bounded_input(
                    input_sender,
                    XAuthorityRoutedInput {
                        request: sophia_protocol::RoutedInputRequest {
                            serial: event.serial,
                            seat: event.seat,
                            device: event.device,
                            time_msec: event.time_msec,
                            target_surface,
                            global_position: Point::default(),
                            local_position: Point::default(),
                            kind: event.kind,
                        },
                        route_lease: None,
                        delivery: Some(delivery),
                        mode: XAuthorityRoutedInputMode::Deliver,
                        origin: sophia_x_authority::XAuthorityRoutedInputOrigin::Physical,
                    },
                    if pressed {
                        sophia_protocol::CapacityClass::Ordered
                    } else {
                        sophia_protocol::CapacityClass::TerminatingBoundary
                    },
                    &mut report.ingress_saturation,
                )? {
                    continue;
                }
                if client_keys.record_routed(key, pressed).is_saturated() {
                    report.ingress_saturation.ledger_discarded =
                        report.ingress_saturation.ledger_discarded.saturating_add(1);
                    continue;
                }
                if pressed {
                    match key_repeat.arm(
                        KeyRepeatTarget {
                            surface: target_surface,
                            seat: event.seat,
                            device: event.device,
                            keycode: evdev_keycode,
                            source_time_msec: event.time_msec,
                        },
                        now_msec,
                        key_repeat_map.evdev_key_repeats(evdev_keycode),
                    ) {
                        sophia_engine::KeyRepeatArmOutcome::Armed
                        | sophia_engine::KeyRepeatArmOutcome::NotRepeatable => {}
                        sophia_engine::KeyRepeatArmOutcome::SeatCapacityExhausted => {
                            return Err("key repeat seat capacity exhausted".into());
                        }
                    }
                }
                report.keys_routed = report.keys_routed.saturating_add(1);
                report.key_targets.push(target_surface);
                if pressed {
                    report
                        .routed_key_presses
                        .push((event.serial, event.time_msec));
                }
                report.deliveries.push(delivery);
            }
