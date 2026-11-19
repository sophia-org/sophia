//! G3-5b section 1: a frame commit's candidate projects through the one
//! checked placement algorithm its render bundle uses. Only the image
//! source differs, a provisional placeholder for the admitted copy, and the
//! ordinary caller's error strings are unchanged.
use super::*;
use sophia_engine::CompositorImageSource;

fn descriptor() -> HeadlessOutput {
    HeadlessOutput {
        id: OutputId::from_raw(2),
        size: Size {
            width: 1920,
            height: 1080,
        },
        scale: 1,
    }
}

/// The bundle's candidate as a frame commit carries it, and its first
/// placement's resource as admission describes the copy.
fn frame_commit(bundle: &ContentRenderBundle) -> (ContentCandidate, ContentResourceBegin) {
    let candidate = ContentCandidate {
        grant: bundle.grant,
        candidate_generation: bundle.candidate_generation,
        output: bundle.output,
        facts_generation: 6,
        pacing_permit: 1,
        interaction_generation: bundle.interaction_generation,
        surfaces: bundle.surfaces.clone(),
        placements: bundle.placements.clone(),
        targets: bundle.targets.clone(),
    };
    let description = bundle
        .resource(bundle.placements[0].resource)
        .unwrap()
        .description()
        .clone();
    (candidate, description)
}

#[test]
fn a_frame_commit_projects_as_its_render_bundle_with_a_provisional_source() {
    let bundle = render_bundle();
    let (candidate, description) = frame_commit(&bundle);
    let mut ordinary =
        project_render_bundle(&bundle, descriptor(), OUTPUT, &[allocation()]).unwrap();
    let provisional =
        project_frame_commit(&candidate, &description, descriptor(), &[allocation()]).unwrap();
    assert!(matches!(
        &provisional.images[0].resource,
        CompositorImageSource::Provisional(admitted) if *admitted == description
    ));
    assert_eq!(
        ordinary.images[0].resource.identity(),
        provisional.images[0].resource.identity(),
        "the placeholder names the copy the lease would"
    );
    ordinary.images[0].resource = provisional.images[0].resource.clone();
    assert_eq!(
        ordinary, provisional,
        "one placement algorithm: only the source differs"
    );
}

#[test]
fn a_repeated_resource_projects_each_placement_from_the_one_copy() {
    let bundle = render_bundle();
    let (mut candidate, description) = frame_commit(&bundle);
    let mut again = candidate.placements[0].clone();
    again.destination_x_px += 2;
    candidate.placements.push(again);
    let projected =
        project_frame_commit(&candidate, &description, descriptor(), &[allocation()]).unwrap();
    assert_eq!(projected.images.len(), 2);
    assert_eq!(
        projected.images[0].resource.identity(),
        projected.images[1].resource.identity()
    );
    assert_ne!(projected.images[0].node, projected.images[1].node);
    assert_eq!(
        projected.images[1].geometry_px.x,
        projected.images[0].geometry_px.x + 2
    );
}

#[test]
fn an_absent_resource_and_an_escaping_placement_keep_the_ordinary_errors() {
    let bundle = render_bundle();
    let (candidate, mut other) = frame_commit(&bundle);
    other.resource.generation += 1;
    assert_eq!(
        project_frame_commit(&candidate, &other, descriptor(), &[allocation()]).err(),
        Some("content placement names an absent resource"),
        "a copy of another resource leaves this placement absent"
    );
    let mut bundle = bundle;
    bundle.placements[0].destination_x_px = 10_000;
    let (candidate, description) = frame_commit(&bundle);
    let expected = Some("content placement escapes its allocation");
    assert_eq!(
        project_render_bundle(&bundle, descriptor(), OUTPUT, &[allocation()]).err(),
        expected
    );
    assert_eq!(
        project_frame_commit(&candidate, &description, descriptor(), &[allocation()]).err(),
        expected
    );
}
