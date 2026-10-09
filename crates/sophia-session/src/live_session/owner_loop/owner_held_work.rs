// What the owner holds itself on this turn. Read just before the authority
// wait, after every phase that can create or retire such work has run.
OwnerHeldWork {
    input: key_repeat.active_seats() != 0
        || client_keys.pending_len() != 0
        || routed_input_motion_held_since.is_some()
        || keyboard_focus_handoff.target().is_some()
        || pointer_focus_handoff.target().is_some()
        || pending_lease_input.holds_input()
        || !physical_policy_inputs.is_empty()
        || floating_pointer_gesture.active()
        || cursor_shake.is_enlarged(),
    input_receipts: !input_delivery.pending.is_empty()
        || !client_key_release_barrier.is_empty(),
    // A due paced repaint is a real deadline; the wait cap already ends on it.
    frames: native_wait.short_service || native_frame_short_service(
        native_frame_service_request.as_ref(), native_wait_only,
        cursor_updates.dirty_since.is_some() && native_scanout.is_some(),
        native_retirement.pending(),
    ),
    output_topology: active_output_topology_preparation.is_some()
        || topology_presentation_deadline.is_some()
        || output_topology_retry_at.is_some_and(|at| at <= Instant::now())
        || pending_hardware_output_publication.is_some()
        || deferred_output_topology_notice.is_some()
        // Waiting without a native owner has no local presentation work.
        // Ordinary maintenance still polls notices and serves control, while
        // retirement above independently requests service until it finishes.
        || (native_scanout.is_some()
            && (output_topology_owner.input_quarantined()
                || startup_topology_recovery_pending)),
    seat: seat_state != sophia_backend_live::LiveSeatState::Active
        || pending_virtual_terminal.is_some()
        || requested_virtual_terminal.is_some()
        || seat_release_started.is_some(),
    shell_interaction: launcher_capture.active()
        || reference_capture.active()
        || chrome_captures.capture(seat).is_some()
        || descriptor_captures.capture(seat).is_some()
        || metadata_shell
            .as_ref()
            .is_some_and(|shell| shell.launcher_busy() || shell.reference_busy()),
    lifecycle: layout.pending.is_some()
        || pending_wm_update.is_some()
        || !committed_session_actions.is_empty()
        || profile_reload_requested
        || wm_restart_requested
        || config_reload_pending
        || logout_requested
        || emergency_exit_requested
        || session_quiescence.is_some()
        || runtime_deadline_key_drain.is_draining()
        || scripting.owner_work_pending()
        || client_render_devices
            .as_ref()
            .is_some_and(render_devices::LiveRenderDeviceCoordinator::owner_work_pending)
        || wm_session
            .as_ref()
            .is_some_and(|wm| wm.control_restart.is_some() || wm.desktop_reload.is_some()),
}
