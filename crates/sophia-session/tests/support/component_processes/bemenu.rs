//! Actual protected C client and real server stores. Geometry and presentation
//! outcomes are supplied here; this does not execute native rendering or policy.
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_session::application_catalog::*;
use sophia_session::shell_component_connections::ComponentConnectionKey;
use sophia_session::shell_component_processes::ShellComponentProcesses;
use std::time::{Duration, Instant};

pub(super) fn exercise(owner: &mut ShellComponentProcesses, key: ComponentConnectionKey) {
    let apps = ["app1", "app2"].map(|name| RegisteredCatalogApplication {
        name: name.into(),
        command: ApplicationLaunchCommand {
            executable: "/bin/true".into(),
            arguments: vec![],
            working_directory: None,
        },
    });
    let catalog = build_application_catalog(
        &sophia_config::ApplicationCatalogConfig {
            name: "protected-client".into(),
            sources: vec![],
            applications: apps.iter().map(|a| a.name.clone()).collect(),
            terminal: None,
            terminal_arguments: vec![],
        },
        &apps,
        &ApplicationCatalogEnvironment {
            search_path: vec![],
            locale: "C".into(),
            current_desktop: vec![],
        },
    )
    .unwrap();
    let catalog = PublishedApplicationCatalog::new(key.grant.connection_epoch, 1, catalog).unwrap();
    let mut publication = owner
        .with_connection(key, |t| NativeCatalogPublication::new(t, tx(1), catalog))
        .unwrap()
        .unwrap();
    assert!(
        owner
            .with_connection(key, |t| publication.service(t))
            .unwrap()
            .unwrap()
    );
    let opening = NativeLauncherOpening {
        grant: key.grant,
        opening: 1,
        output: ContentOutputId {
            id: 1,
            generation: 1,
        },
        catalog_generation: 1,
        state_revision: 1,
    };
    owner
        .with_connection(key, |t| {
            t.publish_content_output_facts(
                tx(2),
                1,
                vec![ContentOutputFactsEntry {
                    output: opening.output,
                    local_width: 800,
                    local_height: 600,
                    scale_numerator: 1,
                    scale_denominator: 1,
                    scale_generation: 1,
                }],
            )
            .unwrap();
            t.publish_native_launcher_opening(tx(3), opening).unwrap();
        })
        .unwrap();
    let mut permit = 0;
    let mut expected_event = None;
    let mut acknowledged = false;
    let mut previous: Option<ContentRenderBundle> = None;
    for (revision, rows) in [(1, 2), (2, 1)] {
        let deadline = Instant::now() + Duration::from_secs(4);
        let bundle = loop {
            assert!(
                Instant::now() < deadline,
                "actual Bemenu candidate timed out: revision={revision} acknowledged={acknowledged}"
            );
            let bundle = owner
                .with_connection(key, |t| {
                    while let Some((_, ack, accepted)) = t.poll_native_launcher_input_ack().unwrap()
                    {
                        assert_eq!(Some(ack.event), expected_event);
                        assert!(accepted);
                        assert!(!acknowledged);
                        acknowledged = true;
                    }
                    let allocations = t.content_allocation_snapshots();
                    let context = ContentCandidateContext {
                        output: opening.output,
                        facts_generation: 1,
                        interaction_generation: 1,
                        allocations: &allocations,
                    };
                    let current = NativeLauncherCandidateContext {
                        opening,
                        state_revision: revision,
                        catalog: publication.published().unwrap().wire(),
                    };
                    t.service_native_launcher_content(context, current, 0)
                        .unwrap();
                    while let Some((_, request)) = t.next_content_allocation_request() {
                        assert_eq!(request.role, 3);
                        assert_eq!(request.operation, 1);
                        assert!(request.desired_width <= 800 && request.desired_height <= 600);
                        let allocation = ContentAllocationSnapshot {
                            native_opening: Some(opening.opening),
                            output: opening.output,
                            allocation: ContentAllocationId {
                                id: request.allocation_request_id,
                                generation: 1,
                            },
                            scale_generation: 1,
                            scale_numerator: 1,
                            scale_denominator: 1,
                            role: 3,
                            edge: request.edge,
                            margins: request.margins,
                            logical: ContentLogicalRect {
                                x: 0,
                                y: 0,
                                width: request.desired_width,
                                height: request.desired_height,
                            },
                            pixel: ContentPixelRect {
                                x: 0,
                                y: 0,
                                width: request.desired_width,
                                height: request.desired_height,
                            },
                            parent: ContentAllocationId::default(),
                            anchor_parent_rect: ContentPixelRect::default(),
                            allowed_reservation_extent: 0,
                        };
                        t.grant_content_allocation(request.allocation_request_id, allocation, &[])
                            .unwrap();
                    }
                    while let Some((transaction, demand)) = t.next_content_demand() {
                        permit += 1;
                        t.grant_content_demand(transaction, demand.output, permit, 0)
                            .unwrap();
                    }
                    t.next_content_submission().map(|(_, generation)| {
                        t.begin_native_launcher_submission(generation, context, current, 0)
                            .unwrap()
                    })
                })
                .unwrap();
            if let Some(bundle) = bundle {
                break bundle;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(bundle.grant, key.grant);
        assert_eq!(bundle.surfaces.len(), 1);
        assert_eq!(bundle.targets.len(), rows);
        assert_eq!(
            bundle.native_launcher.as_ref().unwrap().state_revision,
            revision
        );
        if let Some(old) = previous.as_ref() {
            assert!(
                acknowledged,
                "filtered candidate follows exact input acknowledgement"
            );
            assert!(bundle.candidate_generation > old.candidate_generation);
            assert_ne!(
                bundle
                    .resource(bundle.placements[0].resource)
                    .unwrap()
                    .bytes(),
                old.resource(old.placements[0].resource).unwrap().bytes()
            );
            assert_eq!(bundle.native_launcher.as_ref().unwrap().selected, 2);
        }
        let bytes = bundle
            .resource(bundle.placements[0].resource)
            .unwrap()
            .bytes();
        assert!(bytes.len() > 1024);
        assert!(
            bytes.chunks_exact(4).any(|pixel| pixel != &bytes[..4]),
            "actual text/rows must change raster pixels"
        );
        owner
            .with_connection(key, |t| {
                // Supplied completion only: no renderer, native queue, or KMS is used.
                t.content_prepared(
                    key.grant,
                    opening.output,
                    bundle.candidate_generation,
                    1,
                    1,
                    0,
                )
                .unwrap();
                t.content_presented(
                    key.grant,
                    opening.output,
                    bundle.candidate_generation,
                    revision,
                    1,
                    1,
                )
                .unwrap();
                let binding = t.install_native_launcher_focus(tx(4 + revision)).unwrap();
                assert_eq!(binding.grant, key.grant);
                assert_eq!(binding.candidate_generation, bundle.candidate_generation);
                if revision == 1 {
                    expected_event = t
                        .issue_native_launcher_input(
                            binding,
                            tx(10),
                            NativeLauncherInputKind::Text,
                            "app2",
                            1000,
                        )
                        .unwrap();
                    assert!(expected_event.is_some());
                }
                t.poll_io().unwrap();
            })
            .unwrap();
        previous = Some(bundle);
    }
    drop(previous);
}

fn tx(value: u64) -> TransactionId {
    TransactionId::from_raw(value)
}
