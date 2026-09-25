{
    if config.input_proof_requested()
        && input_delivery.events_expected != input_delivery.events_flushed
    {
        return Err(format!(
            "persistent live session completed with unflushed X11 input: expected={} flushed={} pending={}",
            input_delivery.events_expected,
            input_delivery.events_flushed,
            input_delivery.pending.len(),
        )
        .into());
    }
    if config.input_proof_requested() && input_delivery.flush_latency.is_none() {
        return Err("persistent live session input proof never observed flushed X11 input".into());
    }
    if config.input_proof_requested() && !input_pixel_change {
        return Err(format!(
            "persistent live session input did not change composed terminal pixels: baseline={injection_checksum:?} final_frame={} final_buffers={} input_surface={input_surface:?} input_surface_pixel_change={input_surface_pixel_change} batches={batches} transactions={transactions}",
            report.checksum,
            scene.buffer_checksum(),
        )
        .into());
    }
    if config.input_proof_requested() && input_presented_latency.is_none() {
        let native_heads = runtime.as_ref().map_or_else(
            || "none".to_owned(),
            LiveProductionVisualRuntime::native_diagnostic,
        );
        return Err(format!(
            "persistent live session input pixels were not presented: change_submission_baseline={input_change_submission_baseline:?} primary_presented_submissions={} native_submissions={} native_callbacks={} native_heads={native_heads}",
            native_scanout
                .as_ref()
                .and_then(|native| native.heads.first())
                .map_or(0, |head| head.presented_submissions),
            native_scanout.as_ref().map_or(0, |native| native.submissions),
            native_scanout
                .as_ref()
                .map_or(0, |native| native.callback_accepted),
        )
        .into());
    }
    if config.expect_physical_text.is_some()
        && native_scanout.as_ref().is_some_and(|native| {
            native.kernel_page_flip_timestamp_missing != 0
                || native.pending_kernel_page_flip_timestamps() != 0
        })
    {
        return Err(
            "physical input proof observed fallback or pending kernel page-flip timestamps".into(),
        );
    }
    if config.expect_physical_text.is_some()
        && native_scanout.is_some()
        && (input_raw_ingress_msec.is_none() || input_presented_ust_usec.is_none())
    {
        return Err(
            "physical input proof did not correlate libinput ingress to its presented frame".into(),
        );
    }
    if config.input_proof_requested() && !input_text_match {
        return Err(
            "persistent live session terminal did not receive the expected text and Return".into(),
        );
    }
    if config.expect_physical_text.is_some()
        && (!physical_text_proof
            .as_ref()
            .is_some_and(PhysicalTextProof::is_complete)
            || !physical_input_completion_reported)
    {
        return Err("persistent live session did not complete exact physical text proof".into());
    }
    if config.expect_physical_pointer
        && (!pointer_pixel_change || physical_pointer_buttons_routed == 0)
    {
        return Err(format!(
            "persistent live session pointer input did not change pixels: baseline={pointer_checksum:?} routed={physical_pointer_routed} buttons={physical_pointer_buttons_routed} observed={physical_pointer_events}"
        )
        .into());
    }
    *failure_phase = crate::diagnostics::SessionFailurePhase::ApplicationProof;
    if config.application_proof_requested() {
        let status =
            primary_exit_status.ok_or("application proof ended before the client exited")?;
        if config.require_client_normal_exit && !status.success() {
            return Err(format!("application did not exit normally: {status}").into());
        }
        if let Some(expected) = config.expect_client_stdout.as_deref()
            && client_stdout != expected.as_bytes()
        {
            return Err(format!(
                "application stdout mismatch: expected_bytes={} received_bytes={}",
                expected.len(),
                client_stdout.len()
            )
            .into());
        }
        if protocol_error_count != 0 {
            return Err(format!(
                "application emitted {protocol_error_count} X protocol errors; first={first_protocol_error:?}"
            )
            .into());
        }
    }
    *failure_phase = crate::diagnostics::SessionFailurePhase::LayoutValidation;
}
