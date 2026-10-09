{
startup_native_recovery_attempted = true;
let current = native_scanout
    .as_mut()
    .ok_or("startup native recovery lost the active scanout")?;
let suspended = runtime
    .as_mut()
    .ok_or("startup native recovery lost the visual runtime")?
    .suspend_native_scanout(current, &outputs, Duration::from_millis(100))?;
native_evidence.observe_settlement(suspended.outcome.drained(), suspended.abandoned_scanouts);
*suspended_renderer_images = Some(capture_renderer_image_handoff(
    runtime
        .as_mut()
        .ok_or("startup native recovery lost the visual runtime")?,
    current,
)?);
close_native_owner!("startup_recovery", RetirementMode::from_suspend(suspended.outcome));
if !native_recovery_allowed!() { continue; }
native_owner_retirement::finish_before_replacement(runtime.as_ref(), native_retirement)?;
// The replacement is resolved against the admitted output profile,
// constructed and resumed by the topology phase, as every rebuild is; the
// retained handoff waits for it there. Readiness completes when that
// replacement presents.
schedule_output_topology_rebuild!("startup_recovery", false);
startup_topology_recovery_pending = true;
crate::session_println!(
    "sophia_live_session_startup schema=4 status=recovery_deferred reason=topology_rebuild attempt=1 cause={} outcome={} drained={} abandoned_scanouts={} retained_images={}",
    recovery_reason.reduced_name(),
    suspended.outcome.reduced_name(),
    suspended.outcome.drained(),
    suspended.abandoned_scanouts,
    suspended_renderer_images.as_ref().map_or(0, |handoff| handoff.len()),
);
std::io::stdout().flush()?;
retired_present_surfaces.clear();
startup_surface_presentations.clear();
startup_content_ready = false;
native_presentation_admitted = false;
startup_required_submissions = None;
input_content_surface = None;
startup_outputs_ready_reported = false;
}
