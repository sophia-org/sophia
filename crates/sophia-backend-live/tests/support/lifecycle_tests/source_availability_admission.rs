//! WM presentation admission against retained-source availability (t306).
//! A DMA-BUF source whose retained image is unavailable on an instance's
//! output has nothing to sample there; the candidate is refused whole, at
//! admission and again at installation (REVIEW-CODEX-06 R3).

use super::*;
use crate::{LivePolicyPresentationRefusal, LiveSourceUnavailableReason};

fn previewing(f: &PresentScene, source: SurfaceId) -> LivePolicyPresentation {
    published(
        10,
        vec![presentation_output(
            f.output,
            PolicyPresentationMode::ReplaceApplications,
        )],
        vec![shown_instance(f.output, 1, 0, source, rect(40, 4, 8, 8))],
        vec![],
    )
}

fn unavailable(
    runtime: &mut LiveProductionVisualRuntime,
    surface: SurfaceId,
    output: Option<OutputId>,
) {
    runtime.source_availability.mark(
        surface,
        LiveSourceUnavailableReason::Pending(sophia_renderer_live::LiveRendererImageId::from_raw(
            31,
        )),
        output.map(|output| [output].into_iter().collect()),
    );
}

#[test]
fn a_dma_buf_source_unavailable_on_the_instance_output_is_refused_whole() {
    let mut f = present_scene();
    unavailable(&mut f.runtime, f.application, Some(f.output));
    let candidate = previewing(&f, f.application);
    assert_eq!(
        f.runtime.validate_policy_presentation(&candidate),
        Err(LivePolicyPresentationRefusal::MissingSource {
            source: f.application
        })
    );
    assert!(
        f.runtime
            .set_policy_presentation(Some(candidate), &f.scene, None)
            .is_err(),
        "installation checks again"
    );
    assert_eq!(f.runtime.policy_presentation(), None);
}

#[test]
fn a_source_unavailable_only_elsewhere_or_drawn_from_cpu_is_admitted() {
    let mut f = present_scene();
    let remote = outputs()[1].id;
    unavailable(&mut f.runtime, f.application, Some(remote));
    let candidate = previewing(&f, f.application);
    assert_eq!(f.runtime.validate_policy_presentation(&candidate), Ok(()));
    assert!(
        f.runtime
            .set_policy_presentation(Some(candidate.clone()), &f.scene, None)
            .unwrap()
    );
    assert_eq!(f.runtime.policy_presentation(), Some(&candidate));

    // A CPU source has no retained image to lose; availability does not
    // refuse it.
    let mut f = present_scene();
    unavailable(&mut f.runtime, f.previewed, None);
    let candidate = previewing(&f, f.previewed);
    assert_eq!(f.runtime.validate_policy_presentation(&candidate), Ok(()));
}

#[test]
fn a_source_lost_between_admission_and_installation_is_refused_at_installation() {
    let mut f = present_scene();
    let candidate = previewing(&f, f.application);
    assert_eq!(f.runtime.validate_policy_presentation(&candidate), Ok(()));
    f.runtime
        .source_availability
        .mark(f.application, LiveSourceUnavailableReason::Lost, None);
    let error = f
        .runtime
        .set_policy_presentation(Some(candidate), &f.scene, None)
        .expect_err("installation must not trust the earlier admission");
    assert_eq!(
        error.to_string(),
        LivePolicyPresentationRefusal::MissingSource {
            source: f.application
        }
        .to_string()
    );
    assert_eq!(
        f.runtime.policy_presentation(),
        None,
        "the installed presentation is unchanged"
    );
}
