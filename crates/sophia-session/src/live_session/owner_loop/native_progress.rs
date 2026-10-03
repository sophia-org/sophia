fn native_frame_service_requires_owner_progress(request: &OutputFrameServiceRequest) -> bool {
    // A waiting software present is owed work too. It stopped being visible in
    // the per-output flags once those became native-only, and without it here
    // the owner would drop to its idle pacing while a present still needed
    // lowering.
    request.presentation_queued
        || request.software_frame_waiting
        || request.preparation_pending
        || request.outputs.iter().any(|output| {
            output.pending_frame || output.native_phase != OutputNativeFramePhase::Idle
        })
}

/// Stateless: once native work is consumed there is no polling tail. Cursor
/// completion and retiring workers retain their independent short fallback.
fn native_frame_short_service(
    request: Option<&OutputFrameServiceRequest>,
    event_wait_only: bool,
    cursor_completion: bool,
    retiring: bool,
) -> bool {
    (!event_wait_only && request.is_some_and(native_frame_service_requires_owner_progress))
        || cursor_completion
        || retiring
}

fn native_frame_service_should_preempt_authority(
    request: &OutputFrameServiceRequest,
    preempted_previous_cycle: bool,
    control_pending: bool,
    control_priority_cycles: u8,
    service_due: bool,
    event_wait_only: bool,
) -> bool {
    // Controls get several owner turns first, but no control class may block
    // native service indefinitely. The service deadline follows owed work,
    // independently of descriptor polling, so watchdogs keep progressing
    // through continuously ready authority traffic. The following
    // owner turn returns to authority traffic.
    const CONTROL_PRIORITY_CYCLES: u8 = 4;

    (!control_pending || control_priority_cycles >= CONTROL_PRIORITY_CYCLES)
        && !preempted_previous_cycle
        && (service_due
            || (!event_wait_only && native_frame_service_requires_owner_progress(request)))
}
