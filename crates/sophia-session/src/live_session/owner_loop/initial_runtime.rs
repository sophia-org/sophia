{
    // TEST_ONLY precedes public launch, but worker setup and the first real
    // framebuffer can still fail. Retain an empty suspended runtime for the
    // ordinary recovery path instead of dropping the session's authorities.
    let may_recover = native_scanout.is_some()
        && config.output_profile.current().availability == sophia_config::DesktopOutputAvailability::Adaptive;
    let initialized = match LiveProductionVisualRuntime::new(&outputs, native_scanout.as_mut()) {
        Ok(runtime) => runtime,
        Err(error) if may_recover => {
            initial_native_activation_failure = Some(error);
            LiveProductionVisualRuntime::new(&outputs, None)?
        }
        Err(error) => return Err(error),
    };
    let mut initialized = initialized
        .with_m4_proof_controls(
            config.m4_first_acquire_delay,
            config.m4_reject_first_present,
            config.m4_diagnose_first_mixed_export,
        )
        .with_surface_chrome_style(initial_border_style);
    initialized.set_transitions_enabled(window_transitions_enabled);
    initialized.set_indicator_publication(
        wm_session.as_ref().and_then(LiveWmSession::indicator_publication),
    );
    *runtime = Some(initialized);
    if initial_native_activation_failure.is_none()
        && let Some(native) = native_scanout.as_mut()
        && let Err(error) = runtime.as_mut().expect("runtime just retained").run_cpu_repaint(
            scene, None, None, LiveProductionCursorPresentation::HardwarePlane, &outputs, native,
        )
    {
        if !may_recover { return Err(error); }
        initial_native_activation_failure = Some(error);
    }
}
