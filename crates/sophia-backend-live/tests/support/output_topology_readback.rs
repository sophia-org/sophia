use super::topology_readback::select_properties;
use super::*;

fn crtc(properties: &[(&str, u64)]) -> std::io::Result<BTreeMap<String, u64>> {
    let mut selected = BTreeMap::new();
    select_properties(
        "crtc",
        &["ACTIVE"],
        &["VRR_ENABLED"],
        properties
            .iter()
            .map(|(name, value)| ((*name).to_owned(), *value)),
        &mut selected,
    )?;
    Ok(selected)
}

#[test]
fn readback_requires_authoritative_properties_and_preserves_optional_absence() {
    assert!(crtc(&[("MODE_ID", 99)]).is_err());
    assert!(crtc(&[("ACTIVE", 1), ("ACTIVE", 0)]).is_err());
    let unsupported = crtc(&[("ACTIVE", 1)]).unwrap();
    let disabled = crtc(&[("ACTIVE", 1), ("VRR_ENABLED", 0)]).unwrap();
    assert_ne!(
        unsupported, disabled,
        "absence must not masquerade as disabled VRR"
    );
    assert_eq!(disabled["crtc.VRR_ENABLED"], 0);
    assert_ne!(
        disabled,
        crtc(&[("ACTIVE", 1), ("VRR_ENABLED", 1)]).unwrap()
    );
}

#[test]
fn readback_compares_display_properties_without_comparing_resource_allocations() {
    let first = crtc(&[("ACTIVE", 1), ("MODE_ID", 55)]).unwrap();
    let restored = crtc(&[("ACTIVE", 1), ("MODE_ID", 88)]).unwrap();
    assert_eq!(
        first, restored,
        "restoration may allocate a different mode blob"
    );
    let plane = |framebuffer, x| {
        let mut values = BTreeMap::new();
        select_properties(
            "plane",
            &["CRTC_ID", "CRTC_X"],
            &["rotation"],
            [
                ("CRTC_ID", 17),
                ("CRTC_X", x),
                ("rotation", 1),
                ("FB_ID", framebuffer),
            ]
            .map(|(name, value)| (name.to_owned(), value)),
            &mut values,
        )
        .unwrap();
        values
    };
    assert_eq!(plane(42, 0), plane(73, 0));
    assert_ne!(
        plane(42, 0),
        plane(73, 1),
        "geometry changes are not restoration"
    );
}
