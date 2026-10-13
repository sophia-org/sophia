use super::*;
use crate::diagnostics::{
    SessionFailurePhase, failure_code, reduced_record, session_failure_record,
};

#[test]
fn overflowing_snapshot_constraints_retain_the_operation_and_exact_inputs() {
    let surface = SurfaceId::new(91, 4);
    let geometry = Rect {
        x: -17,
        y: 20,
        width: 1280,
        height: 720,
    };
    let mut layout = PersistentLiveLayout::default();
    let mut batch = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(1));
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(1);
    batch.client = Some(client);
    add_test_surface_route(&mut batch, surface, client);
    batch
        .presentation_intents
        .push(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Request,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 1,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: Some(Size {
                    width: i32::MAX,
                    height: 720,
                }),
            },
            generation: 1,
        });
    layout.observe_authority_batch(&batch);
    let chrome = sophia_engine::SurfaceChromeStyle::default();
    assert!(chrome.clearance() > 0);
    let error = public_policy_surface_snapshots(
        &layout,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        chrome,
    )
    .unwrap_err();
    assert_eq!(failure_code(error.as_ref()), "chrome_coordinate_overflow");
    assert!(
        error
            .source()
            .unwrap()
            .is::<sophia_engine::ChromeLayoutError>()
    );
    let record = format!("sophia_wm_geometry_failure schema=1 status=failed {error}");
    assert_eq!(reduced_record(&record).as_deref(), Some(record.as_str()));
    for field in [
        "operation=snapshot_constraints",
        "origin=authority_facts",
        "surface=91",
        "surface_generation=4",
        "x=-17",
        "width=1280",
        "max_width=2147483647",
        "max_height=720",
        "min_width=none",
    ] {
        assert!(
            record.split_whitespace().any(|v| v == field),
            "missing {field}: {record}"
        );
    }
    let summary = session_failure_record(SessionFailurePhase::WindowManagement, error.as_ref());
    assert!(summary.contains("failure_code=chrome_coordinate_overflow"));
    assert_eq!(reduced_record(&summary).as_deref(), Some(summary.as_str()));
}

#[test]
fn geometry_failure_reduction_drops_metadata_and_invalid_numeric_fields() {
    let record = "sophia_wm_geometry_failure schema=1 status=failed origin=wm_proposal operation=proposal_content failure_code=chrome_coordinate_overflow surface=2 surface_generation=3 output=1 x=-2147483648 width=2147483647 max_width=none title=private clipboard=private pid=123 namespace=4 x=2147483648 output=garbage origin=private operation=private failure_code=private";
    let reduced = reduced_record(record).unwrap();
    assert_eq!(
        reduced,
        "sophia_wm_geometry_failure schema=1 status=failed origin=wm_proposal operation=proposal_content failure_code=chrome_coordinate_overflow surface=2 surface_generation=3 output=1 x=-2147483648 width=2147483647 max_width=none"
    );
}

#[test]
fn bare_chrome_failures_have_stable_codes() {
    use sophia_engine::ChromeLayoutError::*;
    for (error, code) in [
        (CoordinateOverflow, "chrome_coordinate_overflow"),
        (AllocationTooSmall, "chrome_allocation_too_small"),
        (InvalidClearance, "chrome_invalid_clearance"),
    ] {
        assert_eq!(failure_code(&error), code);
    }
}

#[test]
fn proposal_and_output_failures_report_distinct_origins() {
    let surface = SurfaceId::new(92, 5);
    let output = OutputId::from_raw(2);
    let bounds = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    let projection = sophia_protocol::PolicyProjectionProposal {
        transaction: TransactionId::from_raw(23),
        connection_epoch: 2,
        request_id: 19,
        base_generation: 1,
        active_output: output,
        outputs: vec![sophia_protocol::PolicyOutputProjection {
            output,
            placements: vec![sophia_protocol::PolicySurfacePlacement {
                surface,
                surface_generation: 1,
                geometry: bounds,
                requested_size: Some(Size {
                    width: 1,
                    height: 1,
                }),
                crop: None,
                transform: sophia_protocol::PolicyTransform::Identity,
                presentation: sophia_protocol::PolicyPresentationState::default(),
            }],
            focus: Some(surface),
        }],
        presentation: None,
        launch_contexts: vec![],
        output_launch_contexts: vec![],
        translation_groups: vec![],
        tab_groups: vec![],
        indicators: vec![],
        output_statuses: vec![],
    };
    for (area, operation, origin, expected_code) in [
        (
            bounds,
            "proposal_content",
            "wm_proposal",
            "chrome_allocation_too_small",
        ),
        (
            Rect {
                x: i32::MAX,
                ..bounds
            },
            "allocation_bounds",
            "output_bounds",
            "chrome_coordinate_overflow",
        ),
    ] {
        let error = reconcile_public_policy_proposal(
            &PersistentLiveLayout::default(),
            &projection,
            &BTreeMap::from([(output, area)]),
            &BTreeMap::from([(output, area)]),
            sophia_engine::SurfaceChromeStyle::default(),
        )
        .err()
        .expect("the chosen geometry must fail at the chrome boundary");
        assert_eq!(failure_code(error.as_ref()), expected_code);
        let record = format!("sophia_wm_geometry_failure schema=1 status=failed {error}");
        assert_eq!(reduced_record(&record).as_deref(), Some(record.as_str()));
        assert!(record.contains(&format!("operation={operation}")));
        assert!(record.contains(&format!("origin={origin}")));
        assert!(record.contains("requested_width=1 requested_height=1"));
    }
}
