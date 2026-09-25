const WM_SESSION: &str = include_str!("../src/live_session/wm/session.rs");
const PUBLIC_POLICY: &str = concat!(
    include_str!("../src/live_session/wm/public_policy.rs"),
    include_str!("../src/live_session/wm/public_policy/output_responses.rs"),
    include_str!("../src/live_session/wm/public_policy/output_publication.rs"),
    include_str!("../src/live_session/wm/public_policy/projection.rs"),
    include_str!("../src/live_session/wm/public_policy/output_facade.rs"),
    include_str!("../src/live_session/wm/public_policy/session_start.rs"),
    include_str!("../src/live_session/wm/public_policy/requests.rs"),
    include_str!("../src/live_session/wm/public_policy/restart.rs"),
    include_str!("../src/live_session/wm/public_policy/work_areas.rs"),
    include_str!("../src/live_session/wm/public_policy/commit_and_proof.rs"),
);

fn offset(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?}"))
}

/// A moved work area must be relaid out before public policy is polled.
///
/// The relayout check once sat below an early return into the public path. Three
/// places set `work_area_relayout_required` and one place read it, and that
/// reader was unreachable in every session running the reference WM.
///
/// The visible cost was window chrome. Chrome clearance changing from zero to
/// two raised the flag, nothing consumed it, and windows stayed placed against
/// the old clearance while their focus ring was drawn against the new one. The
/// ring is drawn outside the window geometry, so it landed outside the output
/// entirely, and the only part visible anywhere was the sliver that crossed into
/// a neighbouring output in root space -- a border from one monitor's window
/// appearing on another monitor, and no border at all on its own.
///
/// `enqueue_relayout` already submits the public request. Only the ordering kept
/// it from being reached.
#[test]
fn a_moved_work_area_is_relaid_out_before_public_policy_is_polled() {
    let relayout = offset(WM_SESSION, "if self.work_area_relayout_required {");
    let public_return = offset(
        WM_SESSION,
        "self.poll_public_request(layout, output, allow_new_cycle)",
    );
    assert!(
        relayout < public_return,
        "the relayout check must precede the public early return, or a public \
         policy never sees a work-area change"
    );

    // The reader is still the only one, so its position is the whole guarantee.
    assert_eq!(
        WM_SESSION
            .matches("if self.work_area_relayout_required {")
            .count(),
        1,
        "a second reader would make the ordering above insufficient"
    );
    // And the capability it reaches genuinely submits to the public policy.
    let enqueue = offset(WM_SESSION, "fn enqueue_relayout(");
    WM_SESSION[enqueue..]
        .find("let public = self.public.as_mut().ok_or(\"public WM state is unavailable\")?;")
        .expect("enqueue_relayout submits through public policy state");

    // Nothing in the public path consumes the flag itself, which is why the
    // ordering above is what makes it reachable at all.
    assert!(
        !PUBLIC_POLICY.contains("work_area_relayout_required = true"),
        "the public path does not raise this flag; it is raised by chrome, \
         commit, and work-area changes and consumed in one place"
    );
}
