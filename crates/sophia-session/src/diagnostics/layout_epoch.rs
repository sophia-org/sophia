//! What a launch or resize epoch is allowed to say in the reduced record.
//!
//! A held epoch and a layout timeout were both being reduced to a bare
//! transaction number, which reads as ordinary progress; the difference
//! between the two is a four-second launch stall. The counts say what an epoch
//! waited on and what it excused, and the epoch's own record carries them
//! precisely because a one-surface epoch cannot otherwise be told from a
//! two-surface one that deferred a sibling.

/// The records that report an epoch's state.
fn record(name: &str) -> bool {
    matches!(
        name,
        "sophia_live_wm"
            | "sophia_live_resize_epoch"
            | "sophia_live_visual_admission"
            | "sophia_live_visual_candidate"
            | "sophia_live_visual_candidate_identity"
            | "sophia_live_surface_admission"
            | "sophia_live_surface_presentation"
            | "sophia_live_surface_geometry"
            | "sophia_live_layout_progress"
    )
}

/// The states an epoch moves through. Fixed vocabulary, on those records only.
pub(super) fn status(name: &str, key: &str, value: &str) -> bool {
    record(name)
        && key == "status"
        && matches!(
            value,
            "held"
                | "layout_timeout"
                | "proposal_busy"
                | "visual_armed"
                | "visual_committed"
                | "queue_committed"
                | "queue_aborted"
                | "admission_extent_primed"
                | "admission_extent_rebased"
                | "recovery_extent_cleared"
                | "recovery_configure_acknowledged"
                | "frontend_configured"
                | "frontend_admitted"
                | "selected"
                | "withdrawn"
        )
}

/// Epoch accounting: counts of surfaces and presents, and the transaction a
/// timeout rolled back to. Numeric only, admitted on any record, since a
/// count carries no client, surface or application content.
pub(super) fn count_key(key: &str) -> bool {
    matches!(
        key,
        "surfaces"
            | "deferred"
            | "matched_surfaces"
            | "staged_presents"
            | "rejected_presents"
            | "recovery_extents"
            | "rollback_configures"
            | "rollback_transaction"
            | "rejected_surfaces"
    )
}
