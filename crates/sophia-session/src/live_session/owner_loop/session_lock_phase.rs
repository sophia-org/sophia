// The session lock's owner-loop transitions (t034, t292). Session owns the
// state; Engine draws the cover; only the authenticator's verdict for the
// current attempt of the current lock ends it.
{
// The fill under any provider image, and the whole screen without one.
const SESSION_LOCK_FILL: sophia_engine::CompositorRgb8 =
    sophia_engine::CompositorRgb8 { red: 0, green: 0, blue: 0 };

macro_rules! begin_session_lock {
    ($reason:literal) => {{
        let k = &config.xkb_config;
        // Built before anything changes, so a keymap that cannot load or a
        // secret page that cannot be locked refuses the lock instead of
        // leaving one half taken.
        let input = sophia_engine::SessionLockKeyboard::new(
            &k.rules,
            &k.model,
            &k.layout,
            &k.variant,
            &k.options,
            &std::env::var_os("LC_ALL")
                .or_else(|| std::env::var_os("LC_CTYPE"))
                .or_else(|| std::env::var_os("LANG"))
                .unwrap_or_else(|| "C".into()),
        )
        .map_err(|error| error.to_string())
        .and_then(|keyboard| {
            crate::session_lock_input::SessionLockInput::new(keyboard)
                .map_err(|_| "secret memory could not be locked".to_owned())
        });
        if !session_unlock_authenticator
            .as_ref()
            .is_some_and(|authenticator| authenticator.available())
        {
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
        } else if let Err(error) = &input {
            crate::session_eprintln!(
                "sophia_live_session_lock schema=1 status=refused reason=lock_input source={} error={error}",
                $reason,
            );
        } else if let Ok(input) = input {
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
                    locked::reset_desktop_keyboard_for_lock(
                        seat,
                        wm_session.as_mut().and_then(|wm| wm.shortcuts.as_mut()),
                        &mut modifiers,
                        &mut launcher_keyboard,
                    );
                    // Keys the lock takes are never counted; what was held
                    // before it is forgotten rather than left stale.
                    keyboard_coverage.forget_all_devices();
                    // Replacing the input zeroes any secret of a lock this
                    // one supersedes.
                    let mut input = input;
                    input.set_chords(lock_chords.clone());
                    session_lock_input = Some(input);
                    // The cover is runtime state that every list consults; a
                    // repaint that cannot be queued now still draws it next.
                    if let Some(runtime) = runtime.as_mut()
                        && let Err(error) = runtime.set_session_lock(
                            Some(sophia_engine::SessionLockCover::fill(epoch, SESSION_LOCK_FILL)),
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

macro_rules! service_lock_provider {
    () => {{
        let lock_device = client_render_devices
            .as_ref()
            .and_then(|devices| devices.shell_gpu_device().ok());
        if !lock_provider_tried
            && let Some(snapshot) = wm_session.as_ref().and_then(|wm| wm.published_output_snapshot())
        {
            lock_provider_tried = true;
            lock_provider = lock_provider::start_session_lock_provider(
                &config,
                session_unlock_authenticator.is_some(),
                &snapshot,
                session_lock.phase(),
                lock_device.clone(),
                owner_wake.notifier(),
            );
        }
        if let Some(provider) = lock_provider.as_mut() {
            use sophia_runtime::lock_files::{LockFileServiceCommand, LockInbound};
            // Images drawn for an earlier lock never show in this one.
            let mut images_changed = lock_frames.lock(session_lock.cover_epoch());
            lock_frames.set_pacing_diagnostics(provider.pacing_enabled());
            // A direct grant follows the render device. What the old process
            // was granted ends now, while it may still be exiting.
            if provider.follow_device(lock_device, Instant::now()) {
                images_changed |= crate::session_lock_succession::revoke_replaced_lock_provider(
                    &mut lock_chords,
                    session_lock_input.as_mut(),
                    &mut lock_frames,
                );
            }
            let events = provider.poll(Instant::now());
            if provider.take_failure() {
                images_changed |= crate::session_lock_succession::revoke_replaced_lock_provider(
                    &mut lock_chords,
                    session_lock_input.as_mut(),
                    &mut lock_frames,
                );
            }
            for event in events {
                match event {
                    sophia_runtime::lock_files::LockFileServiceEvent::Connected {
                        connection_epoch,
                        chords,
                    } => {
                        images_changed |= lock_frames.connected(connection_epoch);
                        lock_chords = crate::session_lock_input::session_lock_chords(&chords);
                        if let Some(input) = session_lock_input.as_mut() {
                            input.set_chords(lock_chords.clone());
                        }
                        crate::session_println!(
                            "sophia_live_lock_provider schema=1 status=connected connection_epoch={connection_epoch} chords={}",
                            chords.len(),
                        )
                    }
                    sophia_runtime::lock_files::LockFileServiceEvent::Disconnected {
                        connection_epoch,
                    } => {
                        images_changed |= lock_frames.disconnected(connection_epoch);
                        lock_chords.clear();
                        if let Some(input) = session_lock_input.as_mut() {
                            input.set_chords(Vec::new());
                        }
                        crate::session_println!(
                            "sophia_live_lock_provider schema=1 status=disconnected connection_epoch={connection_epoch}",
                        )
                    }
                    // Consumed by the provider; never handed on.
                    sophia_runtime::lock_files::LockFileServiceEvent::Retired { .. } => {}
                    sophia_runtime::lock_files::LockFileServiceEvent::ConnectionRejected {
                        message,
                    }
                    | sophia_runtime::lock_files::LockFileServiceEvent::Failed { message } => {
                        crate::session_eprintln!(
                            "sophia_live_lock_provider schema=1 status=connection_failed error={message}",
                        )
                    }
                    sophia_runtime::lock_files::LockFileServiceEvent::Inbound {
                        connection_epoch,
                        inbound,
                    } => match inbound {
                        LockInbound::Negotiated { .. } => {}
                        LockInbound::ResourceReady {
                            resource,
                            width_px,
                            height_px,
                            pixels,
                        } => lock_frames.resource_ready(
                            connection_epoch,
                            resource,
                            width_px,
                            height_px,
                            pixels,
                        ),
                        LockInbound::ResourceRetired(resource) => {
                            lock_frames.resource_retired(connection_epoch, resource)
                        }
                        LockInbound::Demand(demand) => {
                            lock_frames.demand(connection_epoch, demand)
                        }
                        LockInbound::Candidate { candidate, .. } => {
                            let (changed, owed) =
                                lock_frames.candidate(connection_epoch, candidate);
                            images_changed |= changed;
                            for outcome in owed {
                                provider.command(LockFileServiceCommand::Outcome(outcome));
                            }
                        }
                    },
                }
            }
            // Presentation paces the provider: a permit only while its
            // allocation shows nothing unretired.
            for demand in lock_frames.permits() {
                provider.command(LockFileServiceCommand::Permit {
                    allocation_id: demand.allocation_id,
                    demand_id: demand.demand_id,
                    expires_after: Duration::from_millis(100),
                });
            }
            if let (Some(runtime), Some(native)) = (runtime.as_ref(), native_scanout.as_ref()) {
                for output in lock_frames.waiting().collect::<Vec<_>>() {
                    let shown = runtime.presented_session_lock_image(native, output);
                    if let Some(outcome) = lock_frames.presented(output, shown) {
                        provider.command(LockFileServiceCommand::Outcome(outcome));
                    }
                }
            }
            // Diagnostic only (SOPHIA_DIAGNOSTIC_LOCK_PACING=1): where each
            // allocation's frames stand, every five seconds.
            if provider.pacing_sample_due(Instant::now()) {
                for pacing in lock_frames.pacing() {
                    crate::session_println!(
                        "{}",
                        crate::session_lock_frames::session_lock_pacing_record(&pacing)
                    );
                }
                let untracked = lock_frames.pacing_untracked_observations();
                if untracked != 0 {
                    crate::session_println!(
                        "{}",
                        crate::session_lock_frames::session_lock_pacing_untracked_record(untracked)
                    );
                }
            }
            if images_changed
                && let Some(runtime) = runtime.as_mut()
                && let Some(cover) = runtime.session_lock()
            {
                let cover = sophia_engine::SessionLockCover {
                    images: lock_frames.images(),
                    ..cover
                };
                if let Err(error) =
                    runtime.set_session_lock(Some(cover), &scene, native_scanout.as_mut())
                {
                    // The cover stays drawn with its previous images; the
                    // next change tries again.
                    crate::session_eprintln!(
                        "sophia_live_lock_provider schema=1 status=repaint_deferred error={error}",
                    );
                }
            }
            let topology_epoch =
                wm_session.as_ref().and_then(|wm| wm.output_authority_topology_epoch());
            if let Some(object) = lock_publication.update(session_lock.phase(), topology_epoch, || {
                wm_session.as_ref().and_then(|wm| wm.published_output_snapshot())
            }) {
                // The diagnostic keeps only allocations the new object names.
                lock_frames.retain_pacing(&object.allocations);
                provider.command(sophia_runtime::lock_files::LockFileServiceCommand::PublishLock(object));
            }
        }
    }};
}

/// Tells the provider what the secret did; never what it holds.
macro_rules! lock_entry {
    ($entry:ident, $empty_after:expr) => {{
        if let (Some(provider), Some(epoch)) = (lock_provider.as_ref(), session_lock.cover_epoch()) {
            provider.command(sophia_runtime::lock_files::LockFileServiceCommand::Entry(
                sophia_protocol::lock_files::LockEntry {
                    lock_epoch: epoch.raw(),
                    entry: sophia_protocol::lock_files::LockEntryKind::$entry,
                    empty_after: $empty_after,
                },
            ));
        }
    }};
}

macro_rules! service_session_lock {
    () => {{
        let applied = input_sender.applied_control_epoch() >= session_lock_input_epoch;
        match session_lock.phase() {
            crate::session_lock::SessionLockPhase::Locking { epoch, .. } => {
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
            crate::session_lock::SessionLockPhase::Unlocking { epoch } if applied => {
                // The cover goes only once input can return with it.
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
                } else if session_lock.observe_unlocked(applied) {
                    // Start a fresh desktop interval; never import the lock's
                    // pressed keys, compose state or lock-toggle changes.
                    locked::reset_desktop_keyboard_for_lock(
                        seat,
                        wm_session.as_mut().and_then(|wm| wm.shortcuts.as_mut()),
                        &mut modifiers,
                        &mut launcher_keyboard,
                    );
                    session_lock_input = None;
                    input_sender.set_synthetic_admitted(true);
                    crate::session_println!(
                        "sophia_live_session_lock schema=1 status=unlocked epoch={}",
                        epoch.raw(),
                    );
                }
            }
            crate::session_lock::SessionLockPhase::Locked { .. }
            | crate::session_lock::SessionLockPhase::Unlocking { .. }
            | crate::session_lock::SessionLockPhase::Unlocked => {}
        }
        if let Some(input) = session_lock_input.as_mut() {
            // Edits reach the provider as entries and chords by their ID,
            // never logged: their count and timing would describe the secret.
            for (edit, empty_after) in input.take_edits() {
                match edit {
                    crate::session_lock_input::SessionLockEdit::Insert => lock_entry!(Insert, empty_after),
                    crate::session_lock_input::SessionLockEdit::Delete => lock_entry!(Delete, empty_after),
                    crate::session_lock_input::SessionLockEdit::Clear => lock_entry!(Clear, empty_after),
                    crate::session_lock_input::SessionLockEdit::Submit => lock_entry!(Submit, empty_after),
                }
            }
            for chord in input.take_chords() {
                if let (Some(provider), Some(epoch)) =
                    (lock_provider.as_ref(), session_lock.cover_epoch())
                {
                    provider.command(sophia_runtime::lock_files::LockFileServiceCommand::Chord(
                        sophia_protocol::lock_files::LockChord {
                            lock_epoch: epoch.raw(),
                            chord,
                        },
                    ));
                }
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
                    Ok(()) => {
                        lock_entry!(Checking, true);
                        crate::session_println!(
                            "sophia_live_session_lock schema=1 status=checking epoch={} attempt={}",
                            attempt.epoch.raw(),
                            attempt.serial,
                        )
                    }
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
                    crate::session_println!(
                        "sophia_live_session_lock schema=1 status=unlocking epoch={} input_epoch={} revoked_leases={revoked}",
                        epoch.raw(),
                        session_lock_input_epoch,
                    );
                }
                crate::session_lock::SessionVerdictOutcome::Failed(attempt, verdict) => {
                    match verdict {
                        crate::session_lock::SessionUnlockVerdict::Unavailable => {
                            lock_entry!(Unavailable, true)
                        }
                        _ => lock_entry!(Failed, true),
                    }
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
        if !matches!(session_lock.phase(),
            crate::session_lock::SessionLockPhase::Unlocking { .. }) {
            service_lock_provider!();
        }
    }};
}

include!("physical_input_phase.rs")
}
