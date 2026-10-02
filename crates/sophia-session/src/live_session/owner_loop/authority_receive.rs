// Readiness and deadline selection for a Session owner with no runnable work.
{
    // A worker replaced during this pass gets the wake before the
    // owner sleeps on it. Attaching rings once, ending this wait.
    if let Some(wm) = wm_session.as_ref() {
        wm.attach_owner_wake(&owner_notifier);
    }
    let held: OwnerHeldWork = include!("owner_held_work.rs");
    let now = Instant::now();
    let maximum = authority_wait_timeout(
        owner_input_work_pending(
            physical_input.is_some(),
            !config.normal_session,
            held,
        ),
        cursor_updates.dirty,
        session_controls.pending_len() != 0
            || explicit_pointer_grabs.pending() != 0,
    );
    let maximum = runtime.as_ref().map_or(maximum, |r| r.frame_deadline_cap_wait(now, maximum));
    // Shell wires are served inline and cannot ring, so their own
    // readiness ends this wait. Each is subscribed only when the
    // next pass turns it: components are visited only while a
    // runtime exists. Records queued after a wire's last turn have
    // no descriptor and keep the short budget.
    let mut shell_wires = Vec::new();
    let mut shell_output_pending = false;
    if let Some(shell) = metadata_shell.as_ref() {
        shell_wires.extend(shell.poll_fds());
        shell_output_pending |= shell.output_pending();
    }
    if let Some(components) = shell_components.as_ref().filter(|_| runtime.is_some()) {
        shell_wires.extend(components.poll_fds());
        shell_output_pending |= components.output_pending();
    }
    let maximum = if shell_output_pending {
        maximum.min(Duration::from_millis(1))
    } else {
        maximum
    };
    owner_wake.receive_with_fds(
        authority_receiver,
        paced_repaint_wait_cap(primary_frame_pacer, paced_repaint_runnable, now, maximum),
        shell_wires,
    )?
}
