//! G3-2: a session owner outside this crate names a combined job's
//! disposition through the public re-export chain, so it can publish
//! Malformed for a mismatch before settlement consumes the result.
#![cfg(all(
    feature = "libdrm-events",
    feature = "gbm-probe",
    feature = "experimental-combined-frame-job"
))]

use sophia_backend_live::{CombinedDisposition, CombinedRenderResult};

/// The owner's reading of a result; exhaustive, so a new disposition fails
/// to compile here before it can be ignored by an owner.
fn owner_reading(result: &CombinedRenderResult) -> &'static str {
    match result.disposition() {
        CombinedDisposition::Rendered => "rendered",
        CombinedDisposition::RenderFailed => "render failed",
        CombinedDisposition::Expired => "expired",
        CombinedDisposition::Stopped => "stopped",
        CombinedDisposition::Mismatch => "malformed",
    }
}

#[test]
fn the_owner_names_every_disposition_from_outside_the_crate() {
    let reading: fn(&CombinedRenderResult) -> &'static str = owner_reading;
    let _ = reading;
    assert_ne!(CombinedDisposition::Mismatch, CombinedDisposition::Expired);
}
