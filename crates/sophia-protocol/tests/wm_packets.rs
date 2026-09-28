//! The neutral WM and chrome packets: a surface-scoped chrome action, a
//! manage request with only blind policy data, and a response converted to a
//! layout transaction. Moved from `protocol/framing.rs`, whose binary also
//! carries the socket framing and so needs the IPC codecs.
use sophia_protocol::*;

#[test]
fn chrome_action_request_is_surface_scoped() {
    let request = ChromeActionRequest {
        surface: SurfaceId::new(9, 4),
        generation: 12,
        kind: ChromeActionKind::CloseSurfaceRequested,
    };

    assert_eq!(request.surface, SurfaceId::new(9, 4));
    assert_eq!(request.generation, 12);
    assert_eq!(request.kind, ChromeActionKind::CloseSurfaceRequested);
}

#[test]
fn wm_manage_request_contains_only_blind_policy_data() {
    let surface = SurfaceId::new(2, 1);
    let workspace = WorkspaceId::from_raw(1);
    let request = WmRequestPacket {
        transaction: TransactionId::from_raw(5),
        kind: WmRequestKind::ManageSurface(WmManageSurface {
            node: layout_node(surface, workspace),
            output: OutputId::from_raw(1),
            workspace,
            bounds: Rect {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            },
        }),
    };

    assert_eq!(request.transaction, TransactionId::from_raw(5));
    let WmRequestKind::ManageSurface(manage) = request.kind else {
        panic!("expected manage request");
    };
    assert_eq!(manage.node.surface, surface);
    assert_eq!(manage.workspace, workspace);
}

#[test]
fn wm_response_converts_to_layout_transaction() {
    let surface = SurfaceId::new(2, 1);
    let workspace = WorkspaceId::from_raw(1);
    let response = WmResponsePacket {
        transaction: TransactionId::from_raw(5),
        commands: vec![
            WmCommand::AssignWorkspace { surface, workspace },
            WmCommand::ConfigureSurface(SurfaceSizeRequest {
                surface,
                size: Size {
                    width: 640,
                    height: 480,
                },
            }),
            WmCommand::FocusSurface(surface),
            WmCommand::RenderSurface(SurfacePlacement {
                surface,
                geometry: Rect {
                    x: 10,
                    y: 20,
                    width: 640,
                    height: 480,
                },
                z_index: 3,
                crop: None,
                transform: Transform::IDENTITY,
            }),
        ],
        timeout_msec: 250,
    };

    let transaction = response.into_layout_transaction();

    assert_eq!(transaction.transaction, TransactionId::from_raw(5));
    assert_eq!(transaction.requested_sizes.len(), 1);
    assert_eq!(transaction.focus, Some(surface));
    assert_eq!(transaction.render_positions.len(), 1);
    assert_eq!(transaction.render_positions[0].z_index, 3);
    assert_eq!(transaction.timeout_msec, 250);
}

fn layout_node(surface: SurfaceId, workspace: WorkspaceId) -> LayoutNodeSnapshot {
    LayoutNodeSnapshot {
        surface,
        workspace,
        kind: LayoutNodeKind::Toplevel,
        placement_preference: SurfacePlacementPreference::Default,
        transient_owner: None,
        capabilities: LayoutNodeCapabilities::STANDARD_TOPLEVEL,
        state: LayoutNodeState::NORMAL,
        constraints: SurfaceConstraints {
            min_size: None,
            max_size: None,
        },
        geometry: Rect {
            x: 0,
            y: 0,
            width: 320,
            height: 200,
        },
        generation: 1,
    }
}
