use sophia_protocol::*;

fn workspace() -> PolicyOverviewWorkspace {
    PolicyOverviewWorkspace {
        output: OutputId::from_raw(1),
        workspace: 17,
        bounds: Rect {
            x: 100,
            y: 0,
            width: 1600,
            height: 1000,
        },
        active: true,
        focus: Some(SurfaceId::new(0, 1)),
        placements: vec![PolicyOverviewPlacement {
            surface: SurfaceId::new(0, 1),
            geometry: Rect {
                x: -300,
                y: 8,
                width: 800,
                height: 984,
            },
        }],
    }
}

#[test]
fn overview_round_trip_preserves_offscreen_geometry_and_empty_workspaces() {
    let first = workspace();
    let empty = PolicyOverviewWorkspace {
        workspace: 18,
        active: false,
        focus: None,
        placements: Vec::new(),
        ..first.clone()
    };
    let expected = vec![first, empty];
    let chunks = encode_wm_overview(&expected, 2, 4).unwrap();
    assert_eq!(chunks[0].ordinal, 4);
    assert_eq!(chunks[1].ordinal, 5);
    assert_eq!(
        chunks[0].data.len(),
        2 * PROJECTION_OVERVIEW_WORKSPACE_RECORD_LEN
    );
    assert_eq!(
        chunks[1].data.len(),
        PROJECTION_OVERVIEW_PLACEMENT_RECORD_LEN
    );
    assert_eq!(decode_wm_overview(&chunks).unwrap(), expected);
}

#[test]
fn duplicate_identity_and_unpublished_focus_are_refused() {
    let first = workspace();
    assert!(encode_wm_overview(&[first.clone(), first.clone()], 1, 0).is_err());
    let mut bad = first.clone();
    bad.placements.push(bad.placements[0]);
    assert!(validate_wm_overview(&[bad]).is_err());
    let mut bad = first.clone();
    bad.focus = Some(SurfaceId::new(1, 1));
    assert!(validate_wm_overview(&[bad]).is_err());
    let mut bad = first;
    bad.active = false;
    assert!(validate_wm_overview(&[bad]).is_err());
}

#[test]
fn truncated_counts_reserved_flags_and_foreign_members_are_refused() {
    let chunks = encode_wm_overview(&[workspace()], 1, 0).unwrap();
    for offset in [40, 44] {
        let mut bad = chunks.clone();
        bad[0].data[offset..offset + 4].copy_from_slice(&2u32.to_le_bytes());
        assert!(decode_wm_overview(&bad).is_err());
    }
    let mut bad = chunks.clone();
    bad[1].data[0..8].copy_from_slice(&2u64.to_le_bytes());
    assert!(decode_wm_overview(&bad).is_err());
    let mut bad = chunks;
    bad[1].data.pop();
    assert!(decode_wm_overview(&bad).is_err());
}
