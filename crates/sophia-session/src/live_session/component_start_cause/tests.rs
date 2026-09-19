#![cfg(test)]

//! Every refusal the component start path can raise must carry a code.
//!
//! These are the literals as the source states them. When one changes, this
//! fails rather than the field quietly degrading to `other` in a session
//! nobody is watching -- which is the failure this whole field exists to end.

use super::{StartCause, classify};

/// The exact messages the start path raises, paired with the code each must
/// receive. Taken from component_session.rs, component_launch.rs, gpu.rs and
/// component_lifecycle.rs.
const REFUSALS: &[(&str, StartCause)] = &[
    // component_session.rs
    (
        "component presentation unavailable or cleanup retained",
        StartCause::Presentation,
    ),
    ("component attempt still owned", StartCause::AttemptOwned),
    ("unknown component selection", StartCause::Selection),
    (
        "component session requires one through three selected roles",
        StartCause::Selection,
    ),
    ("component launch evidence missing", StartCause::Evidence),
    (
        "component launch evidence requires a connected attempt",
        StartCause::Evidence,
    ),
    ("stale component launch evidence", StartCause::Evidence),
    // component_launch.rs
    (
        "component executable must be absolute",
        StartCause::LaunchSpec,
    ),
    (
        "bar component requires a positive panel allowance",
        StartCause::LaunchSpec,
    ),
    (
        "component launch requires a reserved nonzero grant",
        StartCause::LaunchSpec,
    ),
    (
        "dock requires an explicit positive reservation",
        StartCause::LaunchSpec,
    ),
    (
        "metadata shell socket requires an absolute parent",
        StartCause::LaunchSpec,
    ),
    ("bar allowance absent", StartCause::LaunchSpec),
    // gpu.rs
    (
        "a denied shell GPU policy carried a device",
        StartCause::GpuGrant,
    ),
    (
        "direct shell GPU access has no admitted render device",
        StartCause::GpuGrant,
    ),
    (
        "shell GPU grant epoch must be nonzero",
        StartCause::GpuGrant,
    ),
    (
        "shell GPU access requires a protection domain",
        StartCause::GpuDomain,
    ),
    (
        "shell render node identity changed before launch",
        StartCause::GpuIdentity,
    ),
    (
        "shell render node identity changed during discovery",
        StartCause::GpuIdentity,
    ),
    (
        "shell render node physical device changed before launch",
        StartCause::GpuIdentity,
    ),
    // component_lifecycle.rs
    (
        "component GPU access requires native client rendering",
        StartCause::GpuGrant,
    ),
];

#[test]
fn every_refusal_the_start_path_raises_carries_a_code() {
    for (message, expected) in REFUSALS {
        let observed = classify(message);
        assert_eq!(
            observed, *expected,
            "{message:?} classified as {:?}, not {expected:?}",
            observed
        );
        assert_ne!(
            observed,
            StartCause::Other,
            "{message:?} reached the table without a code"
        );
    }
}

#[test]
fn a_message_that_gains_a_trailing_detail_keeps_its_code() {
    // Several of these interpolate an io error after the phrase. The code must
    // survive that, or a real failure reports `other` precisely when the extra
    // detail would have been most useful.
    assert_eq!(
        classify("shell render node physical identity: No such file or directory"),
        StartCause::GpuIdentity
    );
    assert_eq!(
        classify("component executable must be absolute: lom"),
        StartCause::LaunchSpec
    );
}

#[test]
fn an_unrecognised_refusal_is_other_rather_than_a_wrong_code() {
    assert_eq!(
        classify("something nobody has written yet"),
        StartCause::Other
    );
    assert_eq!(classify(""), StartCause::Other);
}

#[test]
fn every_variant_appears_in_the_admitted_token_list() {
    let variants = [
        StartCause::Presentation,
        StartCause::AttemptOwned,
        StartCause::Selection,
        StartCause::LaunchSpec,
        StartCause::GpuGrant,
        StartCause::GpuIdentity,
        StartCause::GpuDomain,
        StartCause::Evidence,
        StartCause::Other,
    ];
    assert_eq!(
        variants.len(),
        StartCause::ALL.len(),
        "a variant was added without admitting its token"
    );
    for variant in variants {
        assert!(
            StartCause::ALL.contains(&variant.as_str()),
            "{variant:?} emits a token reduction does not admit"
        );
    }
}
