//! The actual component pass, with only native admission/completion supplied.
use super::*;

pub(super) fn service(
    h: &mut Harness,
    custody: &mut protected::Custody,
    key: ComponentConnectionKey,
    scene: &sophia_backend_live::LiveProductionCpuScene,
    output: HeadlessOutput,
    root: Rect,
    pixels: &mpsc::Sender<ContentResourceLease>,
) -> Option<u64> {
    let mut submitted = None;
    {
        let (runtime, mut admit) = h.backend.split_admission();
        measure(Stage::ComponentPass, || {
            component_service::service_components_with_bar_content(
                &mut h.owner,
                &mut h.catalog,
                runtime,
                scene,
                None,
                &[output],
                &[(output.id, root)],
                &mut custody.wm,
                true,
                &mut h.launches,
                &mut custody.children,
                &h.config,
                Path::new("/nonexistent-t100-xauthority"),
                &mut custody.admission_started,
                &mut custody.auth.coordinator,
                &custody.wake,
                &mut custody.applications,
                |bar, transport, runtime, scene, native, outputs, bounds, root| {
                    bar.service_content_to(
                        transport,
                        runtime,
                        scene,
                        native,
                        outputs,
                        bounds,
                        root,
                        |runtime, _native, frame, layer| {
                            assert!(
                                submitted.is_none(),
                                "one output may have only one pending candidate"
                            );
                            assert_eq!(frame.images.len(), 1);
                            let lease = frame.images[0]
                                .resource
                                .shell()
                                .expect("shell image")
                                .clone();
                            assert_eq!(
                                lease.description().resource,
                                resource(frame.candidate_generation)
                            );
                            pixels.send(lease).unwrap();
                            submitted = Some(frame.candidate_generation);
                            admit(runtime, frame, layer)
                        },
                    )
                },
            )
        })
        .unwrap();
    }
    assert_eq!(
        h.owner.phase(key).unwrap(),
        ComponentConnectionPhase::Connected
    );
    if submitted.is_some() {
        measure(Stage::FixtureTarget, || {
            h.backend.simulate_submit(output.id);
            h.backend.simulate_completion(output.id);
        });
    }
    submitted
}
