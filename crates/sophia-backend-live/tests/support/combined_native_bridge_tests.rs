#![cfg(test)]
//! G3-5b controls on the bridge the Session row drives, over the combined
//! fixture as the native and the composition target: one composition, then
//! a plan for exactly the admitted copy of that composition under the
//! reservation that issued it; the owner facts' declines; and the declared
//! composition fault (Q1), which proves propagation, not reachability.
use super::*;
use crate::production_visual_runtime::lifecycle_tests::resources::{shell_frame, upload};
use crate::{
    CombinedComposeRefusal, CombinedComposition, CombinedIneligible, LiveProductionCpuScene,
    LiveProductionCursorPresentation, LiveProductionVisualRuntime, LiveShellContentFrame,
    LiveShellContentLayer, ShellContentNative,
};
use sophia_engine::{CompositorImageSource, HeadlessOutput};
use sophia_protocol::{ContentGrant, ContentLimits, ContentResourceBegin, ContentResourceId};
use std::time::{Duration, Instant};

const SHELL: LiveShellContentLayer = LiveShellContentLayer::Shell;
const WAIT: Duration = Duration::from_secs(10);

fn outputs() -> [HeadlessOutput; 2] {
    [1, 2].map(|raw| HeadlessOutput {
        id: OutputId::from_raw(raw),
        ..HeadlessOutput::deterministic()
    })
}

/// A visual runtime on `outputs`, an empty scene, and a Shell candidate on
/// `outputs[0]` as the job sees it at dispatch: placed by its admitted
/// description, no pixels.
struct Candidate {
    runtime: LiveProductionVisualRuntime,
    scene: LiveProductionCpuScene,
    frame: LiveShellContentFrame,
    admitted: ContentResourceBegin,
    _store: sophia_runtime::ContentResourceStore,
}

fn candidate(outputs: &[HeadlessOutput]) -> Candidate {
    let grant = ContentGrant {
        connection_epoch: 200,
        content_grant_epoch: 200,
    };
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant)).unwrap();
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let mut frame = shell_frame(outputs[0], 1, upload(&mut store, grant, resource));
    let admitted = frame.images[0]
        .resource
        .shell()
        .unwrap()
        .description()
        .clone();
    for image in &mut frame.images {
        let description = image.resource.shell().unwrap().description().clone();
        image.resource = CompositorImageSource::Provisional(description);
    }
    Candidate {
        runtime: LiveProductionVisualRuntime::new(outputs, None).unwrap(),
        scene: LiveProductionCpuScene::new(outputs[0].size),
        frame,
        admitted,
        _store: store,
    }
}

impl Candidate {
    /// The row's composition: the hardware-plane cursor, not shown.
    fn compose(
        &self,
        fixture: &CombinedNativeFixture,
    ) -> Result<CombinedComposition, CombinedComposeRefusal> {
        ShellContentNative::compose_combined_frame(
            fixture,
            &self.runtime,
            &self.scene,
            SHELL,
            &self.frame,
            LiveProductionCursorPresentation::HardwarePlane,
            None,
        )
    }
}

fn ineligible(composed: Result<CombinedComposition, CombinedComposeRefusal>) -> CombinedIneligible {
    match composed {
        Err(CombinedComposeRefusal::Ineligible(reason)) => reason,
        Err(other) => panic!("declined for another reason: {other:?}"),
        Ok(_) => panic!("composed"),
    }
}

#[test]
fn an_eligible_composition_plans_exactly_its_admitted_copy() {
    let outputs = outputs();
    let mut fixture = CombinedNativeFixture::new(&outputs);
    let candidate = candidate(&outputs);
    let composition = candidate
        .compose(&fixture)
        .expect("the lane candidate composes");
    assert_eq!(fixture.composes(), 1);
    assert_eq!(composition.output(), outputs[0].id);
    let (head, _inbox) = ShellContentNative::reserve_combined_frame(&mut fixture, outputs[0].id)
        .expect("the serving facade reserves");
    let frame = ShellContentNative::allocate_combined_frame(&mut fixture);
    let (_plan, dispatch) = ShellContentNative::plan_combined_frame(
        &mut fixture,
        composition,
        &head,
        frame,
        &candidate.admitted,
        Instant::now() + WAIT,
    )
    .expect("the composed candidate's admitted copy plans");
    assert_eq!(
        dispatch.frame(),
        frame,
        "the plan is the frame it was given"
    );
    assert_eq!(dispatch.accept(9).job, 9);
    assert_eq!(dispatch.accept(9).frame, frame);
}

#[test]
fn a_plan_refuses_another_copy_and_another_head() {
    let outputs = outputs();
    let mut fixture = CombinedNativeFixture::new(&outputs);
    let candidate = candidate(&outputs);
    let (head, _inbox) = ShellContentNative::reserve_combined_frame(&mut fixture, outputs[0].id)
        .expect("the serving facade reserves");
    let mut other = candidate.admitted.clone();
    other.resource.generation += 1;
    let frame = ShellContentNative::allocate_combined_frame(&mut fixture);
    let composition = candidate.compose(&fixture).unwrap();
    assert!(
        ShellContentNative::plan_combined_frame(
            &mut fixture,
            composition,
            &head,
            frame,
            &other,
            Instant::now() + WAIT,
        )
        .is_none(),
        "an admitted copy that is not the composed candidate is a refused send"
    );
    let (elsewhere, _other_inbox) =
        ShellContentNative::reserve_combined_frame(&mut fixture, outputs[1].id)
            .expect("the other output's facade reserves");
    let composition = candidate.compose(&fixture).unwrap();
    assert!(
        ShellContentNative::plan_combined_frame(
            &mut fixture,
            composition,
            &elsewhere,
            frame,
            &candidate.admitted,
            Instant::now() + WAIT,
        )
        .is_none(),
        "a composition on one head never plans under another head's reservation"
    );
    assert_eq!(fixture.composes(), 2);
    for issued in [&head, &elsewhere] {
        assert!(
            matches!(
                ShellContentNative::cancel_combined_frame(&mut fixture, issued),
                Ok(true)
            ),
            "a refused send leaves its reservation for the one cancel"
        );
    }
}

#[test]
fn unknown_owner_facts_decline_the_lane() {
    let outputs = outputs();
    let mut fixture = CombinedNativeFixture::new(&outputs);
    let candidate = candidate(&outputs);
    fixture.heads[0].enabled = false;
    assert_eq!(
        ineligible(candidate.compose(&fixture)),
        CombinedIneligible::OwnerFactsUnknown,
        "no enabled head serves the output, so its facts cannot be stated"
    );
}

#[test]
fn an_exporter_capture_request_declines_only_its_output() {
    let outputs = outputs();
    let mut fixture = CombinedNativeFixture::new(&outputs);
    let candidate = candidate(&outputs);
    // The unrelated output's request first: the candidate's output composes.
    let head = &fixture.heads[1];
    let expected = fixture
        .native_owner
        .frame(head.output.id, head.head, head.target_generation, 1);
    fixture.exporters[1].request_combined_capture_for_control(expected);
    assert!(
        candidate.compose(&fixture).is_ok(),
        "a capture of another output is not this output's fact"
    );
    let head = &fixture.heads[0];
    let expected = fixture
        .native_owner
        .frame(head.output.id, head.head, head.target_generation, 1);
    fixture.exporters[0].request_combined_capture_for_control(expected);
    assert_eq!(
        ineligible(candidate.compose(&fixture)),
        CombinedIneligible::CaptureActive
    );
}

#[test]
fn a_declared_compose_fault_comes_back_unclassified_after_one_composition() {
    let outputs = outputs();
    let mut fixture = CombinedNativeFixture::new(&outputs);
    let candidate = candidate(&outputs);
    fixture.declare_compose_fault("declared compose fault");
    match candidate.compose(&fixture) {
        Err(CombinedComposeRefusal::Unclassified(error)) => {
            assert_eq!(error.to_string(), "declared compose fault");
        }
        Err(other) => panic!("the declared fault came back as {other:?}"),
        Ok(_) => panic!("the declared fault composed"),
    }
    assert_eq!(fixture.composes(), 1, "the real composition ran once");
}
