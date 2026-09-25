use super::*;
use std::collections::BTreeMap;

#[test]
fn startup_frame_barrier_is_scoped_to_intersecting_outputs() {
    let focused = Rect {
        x: 100,
        y: 50,
        width: 640,
        height: 480,
    };
    let primary = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    let secondary = Rect {
        x: 1920,
        y: 0,
        width: 1920,
        height: 1080,
    };

    assert!(rects_intersect(focused, primary));
    assert!(!rects_intersect(focused, secondary));
    assert_eq!(startup_submission_requirement(4, 3, true), 4);
    assert_eq!(startup_submission_requirement(1, 1, true), 2);
    assert_eq!(startup_submission_requirement(1, 0, false), 0);
}

#[test]
fn startup_submission_requirements_follow_head_identity_across_reordering() {
    let first = sophia_engine::RenderHeadId::from_raw(11);
    let second = sophia_engine::RenderHeadId::from_raw(22);
    let required = BTreeMap::from([
        (
            first,
            StartupHeadRequirement {
                submission: 4,
                content_frame: 7,
            },
        ),
        (
            second,
            StartupHeadRequirement {
                submission: 9,
                content_frame: 12,
            },
        ),
    ]);

    assert_eq!(
        startup_required_submission_for_head(Some(&required), second),
        Some(StartupHeadRequirement {
            submission: 9,
            content_frame: 12,
        }),
    );
    assert_eq!(
        startup_required_submission_for_head(Some(&required), first),
        Some(StartupHeadRequirement {
            submission: 4,
            content_frame: 7,
        }),
    );
    assert_eq!(
        startup_required_submission_for_head(
            Some(&required),
            sophia_engine::RenderHeadId::from_raw(33),
        ),
        None,
    );
}

#[test]
fn a_flip_carrying_a_precontent_composition_is_not_startup_presented() {
    // The recorded shape of every latency session before this predicate:
    // visual detail pinned the requirement while frame 7 was the newest
    // composition anywhere in the head's pipeline, a render already under way
    // finished into submission 3 carrying that older frame, and the barrier
    // passed while the glass still showed the pre-content picture.
    let stale = StartupOutputEvidence {
        required_submission: 3,
        presented_submissions: 3,
        required_content_frame: 7,
        presented_content_frame: 7,
        callbacks: 3,
        synchronous_modeset: false,
    };
    assert!(!all_startup_outputs_presented(&[stale]));

    // The same head once a composition planned at or after the content
    // reaches the glass.
    let fresh = StartupOutputEvidence {
        presented_content_frame: 9,
        presented_submissions: 4,
        ..stale
    };
    assert!(all_startup_outputs_presented(&[fresh]));

    // A head the focused surface does not intersect owes no submission and
    // never advances its content for it; demanding newness there would wait
    // on a blank output forever.
    let blank = StartupOutputEvidence {
        required_submission: 0,
        presented_submissions: 2,
        required_content_frame: 5,
        presented_content_frame: 5,
        callbacks: 2,
        synchronous_modeset: false,
    };
    assert!(all_startup_outputs_presented(&[fresh, blank]));
}
