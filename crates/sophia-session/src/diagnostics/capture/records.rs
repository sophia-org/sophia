/// The source is Sophia's own evidence callback, never a mixed child-output
/// pipe. Keep numeric measurements and a small vocabulary; reject payload fields.
pub fn reduced_record(line: &str) -> Option<String> {
    let mut fields = line.split_whitespace();
    let name = fields.next()?;
    if !name.starts_with("sophia_") || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return None;
    }
    let mut result = name.to_owned();
    for field in fields {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        if !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
            continue;
        }
        if [
            "xid",
            "namespace",
            "pid",
            "title",
            "class",
            "path",
            "payload",
            "handle",
            "text",
            "cookie",
            "display",
            "detail",
            "error",
            "name",
            "uri",
            "clipboard",
            "notification",
            "icon",
        ]
        .iter()
        .any(|part| key.contains(part))
        {
            continue;
        }
        if super::x_lifecycle::record(name) {
            if super::x_lifecycle::field(name, key, value) {
                result.push(' ');
                result.push_str(field);
            }
            continue;
        }
        if super::input_device::record(name) {
            if super::input_device::field(name, key, value) {
                result.push(' ');
                result.push_str(field);
            }
            continue;
        }
        if super::xtest::record(name) {
            if super::xtest::field(name, key, value) {
                result.push(' ');
                result.push_str(field);
            }
            continue;
        }
        if super::selection::record(name) {
            if super::selection::field(name, key, value) {
                result.push(' ');
                result.push_str(field);
            }
            continue;
        }
        if super::wm_pointer::record(name) {
            if super::wm_pointer::field(name, key, value) {
                result.push(' ');
                result.push_str(field);
            }
            continue;
        }
        if matches!(
            name,
            "sophia_live_session_input_recovery"
                | "sophia_live_session_input_delivery"
                | "sophia_live_session_completion"
        ) {
            if super::recovery::field(key, value) {
                result.push(' ');
                result.push_str(field);
            }
            continue;
        }
        if super::shell_component::record(name) {
            if super::shell_component::field(name, key, value) {
                result.push(' ');
                result.push_str(field);
            }
            continue;
        }
        if super::shell_action::record(name) {
            if super::shell_action::field(name, key, value) {
                result.push(' ');
                result.push_str(field);
            }
            continue;
        }
        if let Some(limit) = match key {
            "major" | "code" => Some(u64::from(u8::MAX)),
            "minor" => Some(u64::from(u16::MAX)),
            "distinct" => Some(64),
            "discarded" | "total" => Some(u64::MAX),
            _ => None,
        } {
            // These are protocol classifications, not application identifiers.
            // Keep their ranges and record scope explicit rather than allowing
            // arbitrary numeric fields through the general measurement filter.
            if name == "sophia_live_session_protocol_error_tally"
                && !value.is_empty()
                && value.bytes().all(|c| c.is_ascii_digit())
                && value.parse::<u64>().is_ok_and(|number| number <= limit)
            {
                result.push(' ');
                result.push_str(field);
            }
            continue;
        }
        if name == "sophia_live_layout_probe"
            && matches!(
                key,
                "status"
                    | "source_image"
                    | "native_generation"
                    | "preference_generation"
                    | "original_stage"
                    | "original_status"
                    | "alternative_status"
                    | "original_errno"
                    | "alternative_errno"
                    | "format"
                    | "original_modifier"
                    | "alternative_modifier"
            )
        {
            if layout_probe_field(key, value) {
                result.push(' ');
                result.push_str(field);
            }
            continue;
        }
        if name == "sophia_live_atomic_test" && matches!(key, "status" | "request_scope" | "errno")
        {
            if atomic_test_field(name, key, value) {
                result.push(' ');
                result.push_str(field);
            }
            continue;
        }
        let measurement = [
            "_msec",
            "_usec",
            "_nsec",
            "_bytes",
            "_kib",
            "_count",
            "_total",
            "_peak",
            "_depth",
            "_capacity",
            "_generation",
            "_epoch",
            "_samples",
        ]
        .iter()
        .any(|suffix| key.ends_with(suffix))
            || matches!(
                key,
                "schema"
                    | "seq"
                    | "generation"
                    | "epoch"
                    | "count"
                    | "samples"
                    | "surface"
                    | "transaction"
                    | "output"
                    | "width"
                    | "height"
                    | "exit_status"
                    | "cpu_registry_buffers"
                    | "cpu_cow_splits"
                    | "frame_slots_leased"
                    | "snapshot_live_entries"
                    | "import_cache_live_entries"
                    | "connection_epoch"
                    | "requests"
                    | "committed"
                    | "restarts"
                    | "devices"
                    | "keyboards"
                    // Owner-loop scheduling counters. Tallies of the loop's own
                    // turns -- how often composition was deferred, how often it
                    // repainted, how many authority batches merged -- with no
                    // client, surface or application content in them.
                    //
                    // They were dropped for ending in no recognised suffix,
                    // which left the scheduler record carrying only its frame
                    // interval. A session could then say it paced at 8.3ms and
                    // not whether it ever hit that cadence, so a halving under
                    // input load could be measured from outside and not
                    // attributed from within.
                    | "authority_batches"
                    | "cpu_compositions"
                    | "coalesced_batches"
                    | "cadence_deferred_batches"
                    | "cadence_repaints"
                    | "merged_batches"
                    | "max_merge_run"
                    // Present-scheduler tallies, admitted for the same reason
                    // and dropped for the same one: they end in no recognised
                    // suffix. Without them a desktop session can say it paced
                    // an invisible client without saying how often, so the
                    // mechanism can be measured on the rig and only asserted
                    // on the machine people actually use.
                    | "paced_skips"
                    | "max_frame_tick_parked"
                    | "frame_tick_overflows"
                    | "max_pending_queued"
                    | "max_total_queued"
                    | "present_rejections"
            )
            || super::layout_epoch::count_key(key);
        let numeric = measurement && !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit());
        let digest = (key == "digest" || key.ends_with("sha256"))
            && value.len() == 64
            && value.bytes().all(|c| c.is_ascii_hexdigit());
        let fixed = matches!(
            value,
            "true"
                | "false"
                | "none"
                | "unknown"
                | "unavailable"
                | "applied"
                | "core"
                | "desktop"
                | "wm"
                | "shell"
                | "loaded"
                | "ready"
                | "starting"
                | "started"
                | "stopped"
                | "failed"
                | "rejected"
                | "accepted"
                | "committed"
                | "complete"
                | "returned"
                | "entering"
                | "preflight"
                | "input_guard"
                | "graphics_takeover"
                | "session"
                | "handoff"
                | "degraded"
                | "restarted"
                | "restart_requested"
                | "reload_requested"
                | "reload_staged"
                | "reload_unchanged"
                | "reload_declined"
                | "activated"
                | "user"
                | "system"
                | "explicit"
                | "packaged-fallback"
                | "normal"
                | "physical"
                | "native"
                | "hagia"
                | "kitty"
                | "queued"
                | "preparing"
                | "quiesced"
                | "requested"
                | "detected"
                | "bounded_cleanup"
                | "owner_loop"
                | "virtual_terminal"
                | "modifier_release_timeout"
                | "quiesce"
                | "request"
                | "disable_timeout"
                | "release_pending"
                | "suspended"
                | "active"
                | "captured"
                | "restored"
                | "discarded"
                | "export_images"
                | "drained"
                | "forced_detach_timeout"
                | "forced_detach_drain_error"
                | "forced_detach_revoked"
        );
        let protocol_status = name == "sophia_live_session_protocol_error_tally"
            && key == "status"
            && matches!(value, "clean" | "compatibility_refusals");
        let quiescence_status = name == "sophia_live_session_quiescence"
            && key == "status"
            && matches!(value, "frontend_drained" | "timed_out");
        let quiescence_reason = name == "sophia_live_session_quiescence"
            && key == "reason"
            && matches!(
                value,
                "logout_complete"
                    | "runtime_deadline"
                    | "startup_application_exit"
                    | "successful_primary_exit"
                    | "input_proof_complete"
                    | "tick_limit"
            );
        let epoch_status = super::layout_epoch::status(name, key, value);
        let allocation_stop = name == "sophia_window_allocation_publisher"
            && ((key == "status" && value == "stopped")
                || (key == "pending_cancelled" && matches!(value, "true" | "false")));
        let failure = key == "failure_code" && super::failure::approved_failure_code(value);
        let failure_phase = name == "sophia_session_failure"
            && key == "phase"
            && super::session_failure::approved_phase(value);
        let panic_site = name == "sophia_session_panic"
            && key == "source_file"
            && value.len() <= 128
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'));
        let panic_line = name == "sophia_session_panic"
            && key == "source_line"
            && value.bytes().all(|c| c.is_ascii_digit());
        let application_capture = name == "sophia_application_capture"
            && ((key == "launch_id" && value.parse::<u64>().is_ok())
                || (key == "status" && value == "incomplete")
                || (key == "reason"
                    && matches!(
                        value,
                        "shutdown_timeout"
                            | "storage_failure"
                            | "setup_failure"
                            | "capture_unavailable"
                    )));
        let reload_reason = name == "sophia_config_reload"
            && key == "reason"
            && matches!(value, "prepare" | "read");
        let application_launch = name == "sophia_application_launch"
            && ((matches!(key, "launch_id" | "exit_code" | "exit_signal")
                && value.parse::<u64>().is_ok())
                || (key == "status" && matches!(value, "spawned" | "exited"))
                || (key == "reason"
                    && matches!(
                        value,
                        "not_found" | "permission_denied" | "resource_limit" | "spawn_failure"
                    )));
        if interaction_field(name, key, value)
            || numeric
            || digest
            || fixed
            || epoch_status
            || protocol_status
            || quiescence_status
            || quiescence_reason
            || allocation_stop
            || failure
            || failure_phase
            || panic_site
            || panic_line
            || application_capture
            || reload_reason
            || application_launch
        {
            result.push(' ');
            result.push_str(field);
        }
    }
    Some(result)
}

fn atomic_test_field(record: &str, key: &str, value: &str) -> bool {
    if record != "sophia_live_atomic_test" {
        return false;
    }
    match key {
        "status" => matches!(value, "Submitted" | "WouldBlock" | "Rejected"),
        "request_scope" => matches!(value, "PageFlip" | "Modeset"),
        "errno" => {
            value == "none"
                || (!value.is_empty()
                    && value.bytes().all(|byte| byte.is_ascii_digit())
                    && value.parse::<i32>().is_ok_and(|errno| errno > 0))
        }
        _ => false,
    }
}

fn layout_probe_field(key: &str, value: &str) -> bool {
    match key {
        "original_stage" => matches!(value, "Atomic" | "Framebuffer"),
        "status" => matches!(
            value,
            "Tested"
                | "RetiredCopy"
                | "PreferenceMatched"
                | "FramebufferRejected"
                | "FramebufferRejectionIneligible"
                | "LayoutMismatch"
                | "MissingRequestEvidence"
                | "SelectionMismatch"
                | "GeometryMismatch"
                | "RequestMismatch"
        ),
        "original_status" | "alternative_status" => {
            value == "none" || atomic_test_field("sophia_live_atomic_test", "status", value)
        }
        "original_errno" | "alternative_errno" => {
            atomic_test_field("sophia_live_atomic_test", "errno", value)
        }
        "format" => value.bytes().all(|byte| byte.is_ascii_digit()) && value.parse::<u32>().is_ok(),
        "source_image"
        | "native_generation"
        | "preference_generation"
        | "original_modifier"
        | "alternative_modifier" => {
            value.bytes().all(|byte| byte.is_ascii_digit()) && value.parse::<u64>().is_ok()
        }
        _ => false,
    }
}

// These records describe delivery and composition, never input contents.
// Scope the vocabulary to its producer so arbitrary child text cannot become
// an approved status or an identifier disguised as a numeric measurement.
fn interaction_field(record: &str, key: &str, value: &str) -> bool {
    if record == "sophia_shell_pointer_binding" {
        match key {
            "reason" => return matches!(value, "none" | "target_continuity_lost"),
            "status" => {
                return matches!(value, "captured" | "activated" | "cancelled" | "consumed");
            }
            "observed_output" | "candidate" | "presentation" => {
                return !value.is_empty()
                    && value.bytes().all(|b| b.is_ascii_digit())
                    && value.parse::<u64>().is_ok();
            }
            _ => {}
        }
    }
    if record == "sophia_live_wm_configuration" {
        return match key {
            "reason" => value == "unavailable_session_slot",
            "missing_slots" => {
                !value.is_empty()
                    && value.split(',').count() <= sophia_protocol::POLICY_MAX_BINDINGS
                    && value.split(',').all(|slot| {
                        !slot.is_empty()
                            && slot.bytes().all(|byte| byte.is_ascii_digit())
                            && slot.parse::<u16>().is_ok_and(|slot| slot != 0)
                    })
            }
            _ => false,
        };
    }
    if record == "sophia_live_visual_admission"
        && key == "status"
        && matches!(value, "armed" | "committed" | "presented" | "retry_pixels")
    {
        return true;
    }
    if record == "sophia_live_visual_progress" && visual_progress_field(key, value) {
        return true;
    }
    if shell_content_gate_field(record, key, value) {
        return true;
    }
    let measurement = match record {
        "sophia_live_input_lease" => {
            matches!(key, "confirmed" | "rejected" | "released" | "stale")
        }
        "sophia_live_explicit_pointer_grab" => matches!(
            key,
            "prepared"
                | "activated"
                | "released"
                | "aborted"
                | "rejected"
                | "deferred"
                | "cancelled"
        ),
        "sophia_live_compositor_chrome_set" => matches!(
            key,
            "eligible_surfaces"
                | "frames"
                | "focused_frames"
                | "unfocused_frames"
                | "focus_rings"
                | "primitives"
                | "clearance"
        ),
        "sophia_live_session_present_feedback" => matches!(key, "ust" | "msc"),
        _ => false,
    };
    if measurement {
        return !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && value.parse::<u64>().is_ok();
    }
    match (record, key) {
        ("sophia_live_session_pointer", "status") => matches!(
            value,
            "motion_observed"
                | "motion_routed"
                | "button_observed"
                | "button_routed"
                | "button_suppressed"
                | "axis_observed"
                | "axis_routed"
                | "axis_batch"
                | "target_routed"
        ),
        ("sophia_live_session_pointer", "reason") => matches!(value, "no_target" | "policy"),
        ("sophia_live_session_input_pipeline", "status") => matches!(
            value,
            "key_observed" | "key_routed" | "key_suppressed" | "focus_applied" | "focus_ready"
        ),
        ("sophia_live_session_input_pipeline", "reason") => value == "no_focus",
        ("sophia_live_session_focus", "status") => value == "cleared",
        ("sophia_live_session_focus", "reason") => value == "active_output_empty",
        ("sophia_live_input_lease", "status") => {
            matches!(value, "quarantined" | "refused" | "release_deferred")
        }
        ("sophia_live_input_lease", "reason") => matches!(
            value,
            "release_timeout"
                | "capacity"
                | "binding_timeout"
                | "held_evidence"
                | "outside_scope"
                | "target_evidence"
                | "device"
                | "output"
                | "control_epoch"
                | "authority_session"
                | "readiness"
                | "identity"
        ),
        ("sophia_live_explicit_pointer_grab", "status") => value == "rejected",
        ("sophia_live_explicit_pointer_grab", "reason") => matches!(
            value,
            "anchor_admission" | "anchor_unmapped" | "anchor_owner" | "no_anchor"
        ),
        ("sophia_live_compositor_chrome_set", "status") => value == "composed",
        ("sophia_live_compositor_chrome_frame", "source") => {
            matches!(value, "present" | "production" | "repaint")
        }
        // Compositor-owned frame placement, the same coordinates the WM chrome
        // record already carries; never a pointer position or client payload.
        ("sophia_live_compositor_chrome_frame", "x" | "y") => bounded_signed_pixel(value),
        ("sophia_live_session_present_feedback", "kind") => matches!(value, "idle" | "complete"),
        ("sophia_live_session_present_feedback", "mode") => {
            matches!(value, "Copy" | "Flip" | "Skip" | "SuboptimalCopy")
        }
        ("sophia_live_session_present", "status") => matches!(value, "retired" | "discarded"),
        ("sophia_live_session_present", "outcome") => {
            matches!(value, "stale_surface" | "invalid_surface" | "timed_out")
        }
        _ => false,
    }
}

fn bounded_signed_pixel(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<i32>().is_ok()
}

// Native shell acceptance reads the bounded lifecycle vocabulary from the
// structured recorder. These values describe compositor-owned outcomes; no
// client text, coordinates or resource identifiers enter through this seam.
fn shell_content_gate_field(record: &str, key: &str, value: &str) -> bool {
    match (record, key) {
        ("sophia_live_shell_gpu", "status") => {
            matches!(value, "granted" | "denied" | "revoked")
        }
        ("sophia_live_shell_content", "status") => matches!(
            value,
            "outputs" | "prepared" | "presented" | "transport_failed" | "presentation_failed"
        ),
        ("sophia_live_shell_content", "outputs") => {
            !value.is_empty()
                && value.bytes().all(|byte| byte.is_ascii_digit())
                && value.parse::<u64>().is_ok()
        }
        ("sophia_live_shell_content", "stage") => matches!(
            value,
            "idle"
                | "resources"
                | "outputs"
                | "allocations"
                | "demands"
                | "candidates"
                | "submission"
                | "projection"
                | "runtime"
                | "prepared"
                | "presentation"
        ),
        _ => false,
    }
}

fn visual_progress_field(key: &str, value: &str) -> bool {
    let number = |text: &str| {
        !text.is_empty() && text.bytes().all(|c| c.is_ascii_digit()) && text.parse::<u64>().is_ok()
    };
    match key {
        "status" => matches!(
            value,
            "enabled" | "content" | "committed_snapshot" | "head_snapshot" | "feedback_ready"
        ),
        "stage" => value == "offered",
        "source" => matches!(
            value,
            "none" | "x_pixmap" | "cpu" | "dma_buf" | "dma_present" | "software_present"
        ),
        "kind" => matches!(value, "complete" | "idle"),
        "head" | "submissions" | "retirements" | "submissions_delta" | "retirements_delta" => {
            number(value)
        }
        "surface_token" => value.len() == 16 && value.bytes().all(|c| c.is_ascii_hexdigit()),
        "pending" | "rendering" | "submitted" | "presented" => {
            if value == "none" {
                return true;
            }
            let mut fields = value.split(':');
            matches!(
                fields.next(),
                Some("cpu" | "mixed_present" | "retained_mixed" | "head_composition")
            ) && fields.next().is_some_and(number)
                && fields.next().is_some_and(|v| v == "none" || number(v))
                && fields.next().is_none()
        }
        _ => false,
    }
}
