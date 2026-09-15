//! Actual submitted/resource stores and transferred render-bundle ownership.
//! This does not construct native scanout or simulate a successful device drain.
use super::*;

fn submitted() -> (ContentEpochPool, ContentRenderBundle) {
    let limits = ContentLimits::prototype(grant());
    let mut epochs = ContentEpochPool::new(limits.max_session_retiring_bytes).unwrap();
    epochs.admit(limits).unwrap();
    upload(epochs.active_mut().unwrap(), 1);
    let (resources, candidates) = epochs.active_parts_mut().unwrap();
    assemble(candidates, resources, 1, &[allocation()]);
    let render = candidates.begin_submission(output(), 1, 4).unwrap();
    (epochs, render)
}

#[test]
fn a_live_epoch_returns_the_actual_backend_owner_without_settling() {
    let (mut epochs, render) = submitted();
    let before = epochs.accounting();
    let render = epochs.finish_after_backend_drop(render).err().unwrap();
    assert_eq!(epochs.accounting(), before);
    assert_eq!(render.resource(resource_id(1)).unwrap().bytes().len(), 8);
    assert_eq!(
        epochs
            .active_candidates()
            .unwrap()
            .submitted_candidate_count(),
        1
    );
    epochs.disconnect();
    assert_eq!(epochs.finish_after_backend_drop(render).ok(), Some(1));
    assert!(epochs.accounting().quiescent());
    assert_eq!(epochs.finish_after_backend_drop(()).ok(), Some(0));
    assert!(epochs.accounting().quiescent());
}

#[test]
fn ending_one_backend_does_not_hide_an_independent_real_pixel_consumer() {
    let (mut epochs, render) = submitted();
    let held = render.resource(resource_id(1)).unwrap().clone();
    epochs.disconnect();
    assert_eq!(epochs.finish_after_backend_drop(render).ok(), Some(1));
    let remaining = epochs.accounting();
    assert_eq!(remaining.candidates, 0);
    assert_eq!(remaining.resources, 1);
    assert_eq!(remaining.retired_epochs, 1);
    assert_eq!(remaining.reserved_bytes, 8);
    assert!(!remaining.quiescent());
    assert_eq!(held.bytes().len(), 8);
    assert_eq!(epochs.finish_after_backend_drop(()).ok(), Some(0));
    assert_eq!(epochs.accounting(), remaining);
    drop(held);
    epochs.collect();
    assert!(epochs.accounting().quiescent());
    assert_eq!(epochs.accounting().grant, grant());
}

#[test]
fn replacement_grant_prevents_global_shutdown_of_the_prior_epoch() {
    let (mut epochs, render) = submitted();
    epochs.disconnect();
    let replacement = ContentGrant {
        connection_epoch: grant().connection_epoch + 1,
        content_grant_epoch: grant().content_grant_epoch + 1,
    };
    epochs.admit(ContentLimits::prototype(replacement)).unwrap();
    let before = epochs.accounting();
    let render = epochs.finish_after_backend_drop(render).err().unwrap();
    assert_eq!(epochs.accounting(), before);
    assert_eq!(before.active_epochs, 1);
    assert_eq!(before.retired_epochs, 1);
    assert_eq!(before.grant, replacement);
    epochs.disconnect();
    assert_eq!(epochs.finish_after_backend_drop(render).ok(), Some(1));
    assert!(epochs.accounting().quiescent());
}
