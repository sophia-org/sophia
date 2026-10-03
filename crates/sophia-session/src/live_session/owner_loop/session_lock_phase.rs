// The session lock's owner-loop transitions (t034, t292). Session owns the
// state; Engine draws the cover; only the authenticator's verdict for the
// current attempt of the current lock ends it.
{
// The fill under any provider image, and the whole screen without one.
const SESSION_LOCK_FILL: sophia_engine::CompositorRgb8 =
    sophia_engine::CompositorRgb8 { red: 0, green: 0, blue: 0 };

macro_rules! begin_session_lock {
    ($reason:literal) => {{
        if session_unlock_authenticator.is_none() {
            // A lock nobody could open is refused, not offered.
            crate::session_eprintln!(
                "sophia_live_session_lock schema=1 status=refused reason=no_authenticator source={}",
                $reason,
            );
        } else if runtime.is_none() || native_scanout.is_none() {
            // Coverage is proven from what the heads retired; without native
            // scanout nothing could prove it.
            crate::session_eprintln!(
                "sophia_live_session_lock schema=1 status=refused reason=no_native_presentation source={}",
                $reason,
            );
        } else {
            match session_lock.lock() {
                Ok(crate::session_lock::SessionLockStart::Started(epoch)) => {
                    // Synthetic input closes before the epoch moves, so no
                    // injection can be stamped into the lock's epoch.
                    input_sender.set_synthetic_admitted(false);
                    // Take the seat first: every lease, grab, capture, held
                    // key and chord of the unlocked desktop ends here.
                    let revoked = advance_application_input_security_epoch(
                        &mut application_route_leases,
                        input_sender,
                        &layout.client_routes,
                        route_lease_release_sender,
                    )?;
                    session_lock_input_epoch = application_route_leases.control_epoch();
                    revoke_floating_pointer_interaction!("session_lock");
                    revoke_chrome_captures!("session_lock");
                    keyboard_focus_handoff = KeyboardFocusHandoffState::default();
                    pointer_focus_handoff = PointerFocusHandoffState::default();
                    deferred_physical_key_timings.clear();
                    flush_all_client_keys!("session_lock");
                    if let Some(wm) = wm_session.as_mut() {
                        wm.observe_keyboard_matching(false);
                    }
                    let k = &config.xkb_config;
                    let keyboard = sophia_engine::SessionLockKeyboard::new(
                        &k.rules,
                        &k.model,
                        &k.layout,
                        &k.variant,
                        &k.options,
                        &std::env::var_os("LC_ALL")
                            .or_else(|| std::env::var_os("LC_CTYPE"))
                            .or_else(|| std::env::var_os("LANG"))
                            .unwrap_or_else(|| "C".into()),
                    )?;
                    session_lock_input =
                        Some(crate::session_lock_input::SessionLockInput::new(keyboard));
                    // The cover is runtime state that every list consults; a
                    // repaint that cannot be queued now still draws it next.
                    if let Some(runtime) = runtime.as_mut()
                        && let Err(error) = runtime.set_session_lock(
                            Some(sophia_engine::SessionLockCover {
                                epoch,
                                fill: SESSION_LOCK_FILL,
                            }),
                            &scene,
                            native_scanout.as_mut(),
                        )
                    {
                        crate::session_eprintln!(
                            "sophia_live_session_lock schema=1 status=repaint_deferred epoch={} error={error}",
                            epoch.raw(),
                        );
                    }
                    crate::session_println!(
                        "sophia_live_session_lock schema=1 status=locking source={} epoch={} input_epoch={} revoked_leases={revoked}",
                        $reason,
                        epoch.raw(),
                        session_lock_input_epoch,
                    );
                }
                Ok(crate::session_lock::SessionLockStart::AlreadyLocked(epoch)) => {
                    crate::session_println!(
                        "sophia_live_session_lock schema=1 status=already_locked source={} epoch={}",
                        $reason,
                        epoch.raw(),
                    );
                }
                Err(error) => {
                    crate::session_eprintln!(
                        "sophia_live_session_lock schema=1 status=refused reason={error:?} source={}",
                        $reason,
                    );
                }
            }
        }
    }};
}

macro_rules! service_session_lock {
    () => {{
        let applied = input_sender.applied_control_epoch() >= session_lock_input_epoch;
        match session_lock.phase() {
            crate::session_lock::SessionLockPhase::Locking { epoch } => {
                let presented = match (runtime.as_ref(), native_scanout.as_ref()) {
                    (Some(runtime), Some(native)) => runtime.presented_session_lock(native),
                    _ => None,
                };
                if session_lock.observe_covered(presented, applied) {
                    crate::session_println!(
                        "sophia_live_session_lock schema=1 status=locked epoch={}",
                        epoch.raw(),
                    );
                }
            }
            crate::session_lock::SessionLockPhase::Unlocking { epoch } => {
                if session_lock.observe_unlocked(applied) {
                    session_lock_input = None;
                    input_sender.set_synthetic_admitted(true);
                    crate::session_println!(
                        "sophia_live_session_lock schema=1 status=unlocked epoch={}",
                        epoch.raw(),
                    );
                }
            }
            crate::session_lock::SessionLockPhase::Locked { .. }
            | crate::session_lock::SessionLockPhase::Unlocked => {}
        }
        if let Some(input) = session_lock_input.as_mut() {
            // Provider delivery is the lock role's (t294); until then the
            // edits and chords are only counted, never retained.
            let edits = input.take_edits().len();
            let chords = input.take_chords().len();
            if edits != 0 || chords != 0 {
                crate::session_println!(
                    "sophia_live_session_lock schema=1 status=input edits={edits} chords={chords}",
                );
            }
            if input.submission().is_some()
                && let Some(authenticator) = session_unlock_authenticator.as_mut()
                && let Some(attempt) = session_lock.begin_attempt()
            {
                let begun = authenticator.begin(
                    attempt,
                    input.submission().map_or("", |secret| secret.as_str()),
                );
                // The secret is the authenticator's now, or nobody's.
                input.settle_submission();
                match begun {
                    Ok(()) => crate::session_println!(
                        "sophia_live_session_lock schema=1 status=checking epoch={} attempt={}",
                        attempt.epoch.raw(),
                        attempt.serial,
                    ),
                    Err(crate::session_lock_input::SessionUnlockUnavailable) => {
                        let outcome = session_lock.settle(
                            attempt,
                            crate::session_lock::SessionUnlockVerdict::Unavailable,
                        );
                        crate::session_eprintln!(
                            "sophia_live_session_lock schema=1 status=unavailable outcome={outcome:?}",
                        );
                    }
                }
            }
        }
        while let Some((attempt, verdict)) = session_unlock_authenticator
            .as_mut()
            .and_then(|authenticator| authenticator.poll())
        {
            match session_lock.settle(attempt, verdict) {
                crate::session_lock::SessionVerdictOutcome::Unlocking(epoch) => {
                    // Lock-time input never reaches an application: the
                    // seat moves to a fresh epoch before the desktop returns.
                    let revoked = advance_application_input_security_epoch(
                        &mut application_route_leases,
                        input_sender,
                        &layout.client_routes,
                        route_lease_release_sender,
                    )?;
                    session_lock_input_epoch = application_route_leases.control_epoch();
                    let cleared = runtime.as_mut().map_or(Ok(true), |runtime| {
                        runtime.set_session_lock(None, &scene, native_scanout.as_mut())
                    });
                    if let Err(error) = cleared {
                        // The cover could not be withdrawn, so the session is
                        // not shown unlocked: it locks again under a new epoch.
                        crate::session_eprintln!(
                            "sophia_live_session_lock schema=1 status=unlock_repaint_failed epoch={} error={error}",
                            epoch.raw(),
                        );
                        begin_session_lock!("unlock_repaint_failed");
                    } else {
                        crate::session_println!(
                            "sophia_live_session_lock schema=1 status=unlocking epoch={} input_epoch={} revoked_leases={revoked}",
                            epoch.raw(),
                            session_lock_input_epoch,
                        );
                    }
                }
                crate::session_lock::SessionVerdictOutcome::Failed(attempt, verdict) => {
                    crate::session_println!(
                        "sophia_live_session_lock schema=1 status=failed epoch={} attempt={} verdict={verdict:?}",
                        attempt.epoch.raw(),
                        attempt.serial,
                    );
                }
                crate::session_lock::SessionVerdictOutcome::Stale => {
                    crate::session_println!(
                        "sophia_live_session_lock schema=1 status=stale_verdict epoch={} attempt={}",
                        attempt.epoch.raw(),
                        attempt.serial,
                    );
                }
            }
        }
    }};
}

include!("physical_input_phase.rs")
}
