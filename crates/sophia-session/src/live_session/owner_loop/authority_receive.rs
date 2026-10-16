// Readiness and deadline selection for a Session owner with no runnable work.
{
    #[cfg(feature = "comparator-profiling")]
    let profile = sophia_runtime::comparator_counters::phases::enter(sophia_runtime::comparator_counters::phases::Phase::Readiness);
    // A worker replaced during this pass gets the wake before the
    // owner sleeps on it. Attaching rings once, ending this wait.
    if let Some(wm) = wm_session.as_ref() {
        wm.attach_owner_wake(&owner_notifier);
    }
    let held: OwnerHeldWork = include!("owner_held_work.rs");
    let now = Instant::now();
    use owner_wake::WaitReason as R;
    let mut plan = authority_wait_plan(
        physical_input.is_some(),
        !config.normal_session,
        held,
        cursor_updates.dirty,
        session_controls.pending_len() != 0
            || explicit_pointer_grabs.pending() != 0,
    );
    plan.cap(
        runtime.as_ref().map_or(plan.timeout, |r| {
            r.frame_deadline_cap_wait(now, plan.timeout)
        }),
        R::FrameDeadline,
    );
    plan.cap(present_clocks.cap_wait(now, plan.timeout), R::Present);
    if let Some(deadline) = native_wait.deadline {
        plan.cap(deadline.saturating_duration_since(now), R::NativeDeadline);
    }
    // Held, hold decisions and sequence timeouts use the same owner clock
    // as routing and per-turn service. Only an actual deadline caps idle;
    // the registry or an open chord alone never requires polling.
    let shortcut = shortcut_wait_cap(
        wm_session.as_ref().and_then(|wm| wm.shortcuts.as_ref()),
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        plan.timeout,
    );
    plan.cap(shortcut, R::Shortcut);
    // Shell wires are served inline and cannot ring, so their own
    // readiness ends this wait. Each is subscribed only when the
    // next pass turns it: components are visited only while a
    // runtime exists. Records queued after a wire's last turn have
    // no descriptor and keep the short budget.
    let mut shell_wires = smallvec::SmallVec::<[rustix::event::PollFd<'_>; 64]>::new();
    let mut shell_output_pending = false;
    let mut component_waits = smallvec::SmallVec::<[_; sophia_config::MAX_SHELL_COMPONENTS]>::new();
    if let Some(shell) = metadata_shell.as_ref() {
        shell.visit_poll_fds(|fd| shell_wires.push(fd));
        shell_output_pending |= shell.output_pending();
        plan.cap(shell.authentication_wait(now), R::ShellAuthentication);
    }
    if let Some(components) = shell_components.as_ref().filter(|_| runtime.is_some()) {
        component_waits = components.subscribe_wait(&mut shell_wires);
        shell_output_pending |= components.output_pending();
        plan.cap(components.authentication_wait(now), R::ShellAuthentication);
    }
    if shell_output_pending {
        plan.pending(R::ShellOutput);
        plan.cap(Duration::from_millis(1), R::ShellOutput);
    }
    plan.cap(
        paced_repaint_wait_cap(primary_frame_pacer, paced_repaint_runnable, now, plan.timeout),
        R::Pacer,
    );
    owner_wake.plan_wait(plan);
    owner_wake.record_native_wait(held.frames, !native_wait.descriptors.is_empty());
    #[cfg(feature = "comparator-profiling")]
    drop(profile);
    owner_wake.receive_observed(
        authority_receiver,
        plan.timeout,
        shell_wires,
        native_wait.descriptors.into_iter()
            .map(|fd| rustix::event::PollFd::from_borrowed_fd(fd, rustix::event::PollFlags::IN)),
        native_scanout.as_ref().map(LiveProductionNativeScanout::completion_progress),
        |fds| { for registration in component_waits { registration.observe(fds); } },
    )?
}
