use sophia_renderer_native_egl::{NativeGlIdentity, NativeGlIdentityState};
use std::sync::Arc;

fn identity(version: &str) -> Arc<NativeGlIdentity> {
    Arc::new(
        NativeGlIdentity::from_strings(
            "AMD".into(),
            "AMD Radeon RX 7900 XTX (radeonsi, navi31)".into(),
            version.into(),
        )
        .unwrap(),
    )
}

#[test]
fn every_string_must_be_present() {
    for (vendor, renderer, version) in [("", "r", "v"), ("a", "", "v"), ("a", "r", "")] {
        assert!(
            NativeGlIdentity::from_strings(vendor.into(), renderer.into(), version.into())
                .is_none()
        );
    }
}

#[test]
fn a_differing_identity_is_kept_as_a_conflict_not_a_replacement() {
    let first = identity("4.6 (Core Profile) Mesa 25.2.4");
    let other = identity("4.6 (Core Profile) Mesa 25.3.0");
    let mut state = NativeGlIdentityState::default();
    assert_eq!(state.observed(), None);
    state.observe(first.clone());
    state.observe(identity("4.6 (Core Profile) Mesa 25.2.4"));
    assert_eq!(state.observed(), Some(&*first));
    state.observe(other.clone());
    let conflict = NativeGlIdentityState::Conflicting {
        first: first.clone(),
        other: other.clone(),
    };
    assert_eq!(state, conflict);
    assert_eq!(state.observed(), None);
    // Later answers, even the first one again, do not resolve it.
    state.observe(first.clone());
    state.observe(identity("4.6 (Core Profile) Mesa 26.0.0"));
    assert_eq!(state, conflict);
}

#[test]
fn merging_never_replaces_and_unobserved_changes_nothing() {
    let first = identity("4.6 Mesa 25.2.4");
    let other = identity("4.6 Mesa 25.3.0");
    let mut held = NativeGlIdentityState::Observed(first.clone());
    held.merge(&NativeGlIdentityState::Unobserved);
    assert_eq!(held, NativeGlIdentityState::Observed(first.clone()));
    // A reopened context starting over cannot replace what was seen.
    held.merge(&NativeGlIdentityState::Observed(other.clone()));
    assert_eq!(
        held,
        NativeGlIdentityState::Conflicting {
            first: first.clone(),
            other: other.clone(),
        }
    );
    let mut empty = NativeGlIdentityState::Unobserved;
    empty.merge(&held);
    assert_eq!(empty, held);
    let mut same = NativeGlIdentityState::Observed(first.clone());
    same.merge(&NativeGlIdentityState::Observed(identity(
        "4.6 Mesa 25.2.4",
    )));
    assert_eq!(same.observed(), Some(&*first));
}
