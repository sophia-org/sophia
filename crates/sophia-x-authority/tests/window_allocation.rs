use sophia_protocol::{NamespaceId, Rect, SurfaceConstraints, SurfaceId, TransactionId};
use sophia_x_authority::*;

const NS: NamespaceId = NamespaceId::from_raw(51);
const WINDOW: XResourceId = XResourceId::new(0x200001, 1);
const SURFACE: SurfaceId = SurfaceId::new(7, 1);
const FORMAT: u32 = sophia_protocol::DRM_FORMAT_ARGB8888;
const DEVICE: XDrmDeviceHint = XDrmDeviceHint {
    major: 226,
    minor: 128,
};

fn runtime() -> XAuthorityRuntime {
    let mut runtime = XAuthorityRuntime::new();
    create(&mut runtime, SURFACE);
    runtime.set_dma_buf_import_formats(vec![XServerFrontendDmaBufImportFormat {
        format: FORMAT,
        modifiers: vec![0, 2, 3],
    }]);
    runtime
}

fn create(runtime: &mut XAuthorityRuntime, surface: SurfaceId) {
    let response = runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(1),
        namespace: NS,
        kind: XAuthorityRequestKind::CreateWindow {
            window: WINDOW,
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
}

fn preferences(runtime: &XAuthorityRuntime, generation: u64) -> XWindowAllocationPreferences {
    XWindowAllocationPreferences {
        generation,
        topology_generation: runtime.output_topology().generation,
        windows: vec![XWindowAllocationPreference {
            surface: SURFACE,
            device: DEVICE,
            identity: None,
            context: None,
            formats: vec![XServerFrontendDmaBufImportFormat {
                format: FORMAT,
                modifiers: vec![3, 2, 2, 9, 0],
            }],
        }],
    }
}

fn dispatch(
    runtime: &mut XAuthorityRuntime,
    namespace: NamespaceId,
    request: XWireRequest,
) -> Vec<XClientOutput> {
    dispatch_x11_wire_request(
        XDispatchContext {
            namespace,
            client_id: 1,
            byte_order: XByteOrder::LittleEndian,
            sequence: 10,
            transaction: TransactionId::from_raw(10),
            major_opcode: X_DRI3_MAJOR_OPCODE,
            injection: XTestAdmission::Absent,
            server_time: 4_242,
        },
        request,
        runtime,
        &mut XAtomTable::new(),
        &mut XPropertyTable::new(),
    )
    .outputs
}

fn query(runtime: &mut XAuthorityRuntime) -> (Vec<u64>, Vec<u64>) {
    match dispatch(
        runtime,
        NS,
        XWireRequest::Dri3GetSupportedModifiers {
            window: WINDOW,
            depth: 32,
            bits_per_pixel: 32,
        },
    )
    .remove(0)
    {
        XClientOutput::Reply(XClientReply::Dri3GetSupportedModifiers {
            window_modifiers,
            screen_modifiers,
            ..
        }) => (window_modifiers, screen_modifiers),
        other => panic!("modifier query failed: {other:?}"),
    }
}

#[test]
fn window_preferences_and_device_hints_never_change_the_screen_contract() {
    let mut runtime = runtime();
    assert_eq!(query(&mut runtime), (vec![], vec![0, 2, 3]));
    let snapshot = preferences(&runtime, 1);
    assert_eq!(
        runtime.update_window_allocation_preferences(snapshot),
        XWindowAllocationUpdate::Applied
    );
    assert_eq!(query(&mut runtime), (vec![0], vec![0, 2, 3]));
    for (hint, expected) in [
        (DEVICE, vec![0]),
        (
            XDrmDeviceHint {
                major: 999,
                minor: 999,
            },
            vec![],
        ),
    ] {
        assert!(
            dispatch(
                &mut runtime,
                NS,
                XWireRequest::Dri3SetDrmDeviceInUse {
                    window: WINDOW,
                    major: hint.major,
                    minor: hint.minor,
                }
            )
            .is_empty()
        );
        assert_eq!(query(&mut runtime), (expected, vec![0, 2, 3]));
    }
}

#[test]
fn an_unpinned_runtime_resolves_only_the_requested_formats_legacy_inventory() {
    let mut runtime = runtime();
    let mut snapshot = preferences(&runtime, 1);
    snapshot.windows[0]
        .formats
        .push(XServerFrontendDmaBufImportFormat {
            format: sophia_protocol::DRM_FORMAT_XRGB8888,
            modifiers: vec![0, 2, 3],
        });
    assert_eq!(
        runtime.update_window_allocation_preferences(snapshot),
        XWindowAllocationUpdate::Applied
    );
    assert_eq!(
        runtime.window_allocation_modifiers(NS, 1, WINDOW, FORMAT),
        vec![0],
        "the unpinned runtime retains its measured legacy LINEAR preference"
    );
    assert!(
        runtime
            .window_allocation_modifiers(NS, 1, WINDOW, sophia_protocol::DRM_FORMAT_XRGB8888)
            .is_empty(),
        "an XRGB window row cannot borrow the ARGB screen inventory"
    );
    assert!(
        runtime
            .window_allocation_modifiers(
                NS,
                1,
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                FORMAT,
            )
            .is_empty(),
        "the root has screen capabilities but no window allocation preference"
    );
}

#[test]
fn hint_namespace_and_surface_lifetime_are_exact() {
    let mut runtime = runtime();
    let snapshot = preferences(&runtime, 1);
    assert_eq!(
        runtime.update_window_allocation_preferences(snapshot),
        XWindowAllocationUpdate::Applied
    );
    let refused = dispatch(
        &mut runtime,
        NamespaceId::from_raw(99),
        XWireRequest::Dri3SetDrmDeviceInUse {
            window: WINDOW,
            major: 226,
            minor: 129,
        },
    );
    assert!(matches!(refused.as_slice(), [XClientOutput::Error(_)]));
    assert_eq!(query(&mut runtime).0, vec![0]);
    runtime.destroy_window(NS, WINDOW).unwrap();
    create(&mut runtime, SurfaceId::new(7, 2));
    assert_eq!(query(&mut runtime).0, Vec::<u64>::new());
}

#[test]
fn invalid_or_stale_preferences_preserve_the_last_accepted_snapshot() {
    let mut runtime = runtime();
    let snapshot = preferences(&runtime, 2);
    assert_eq!(
        runtime.update_window_allocation_preferences(snapshot.clone()),
        XWindowAllocationUpdate::Applied
    );
    assert_eq!(
        runtime.update_window_allocation_preferences(preferences(&runtime, 1)),
        XWindowAllocationUpdate::Stale
    );
    let mut bad = snapshot.clone();
    bad.generation = 3;
    bad.windows.push(bad.windows[0].clone());
    assert_eq!(
        runtime.update_window_allocation_preferences(bad),
        XWindowAllocationUpdate::Invalid
    );
    let mut bad = snapshot;
    bad.generation = 3;
    bad.topology_generation += 1;
    assert_eq!(
        runtime.update_window_allocation_preferences(bad),
        XWindowAllocationUpdate::Stale
    );
    assert_eq!(query(&mut runtime).0, vec![0]);
    let mut topology = runtime.output_topology().clone();
    topology.generation += 1;
    runtime.update_output_topology(topology).unwrap();
    assert_eq!(query(&mut runtime), (vec![], vec![0, 2, 3]));
}

#[test]
fn dri3_version_never_exceeds_the_clients_request() {
    let mut runtime = runtime();
    for (requested, expected) in [
        ((0, 9), (0, 9)),
        ((1, 0), (1, 0)),
        ((1, 2), (1, 2)),
        ((1, 3), (1, 3)),
        ((1, 4), (1, 3)),
        ((2, 0), (1, 3)),
    ] {
        let response = dispatch(
            &mut runtime,
            NS,
            XWireRequest::Dri3QueryVersion {
                major_version: requested.0,
                minor_version: requested.1,
            },
        );
        assert!(
            matches!(response.as_slice(), [XClientOutput::Reply(XClientReply::Dri3QueryVersion {
            major_version, minor_version, ..
        })] if (*major_version, *minor_version) == expected)
        );
    }
}

#[test]
fn drm_device_hint_decodes_both_byte_orders_and_rejects_wrong_length() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let u32bytes = |v: u32| match byte_order {
            XByteOrder::LittleEndian => v.to_le_bytes(),
            XByteOrder::BigEndian => v.to_be_bytes(),
        };
        let length = match byte_order {
            XByteOrder::LittleEndian => 4u16.to_le_bytes(),
            XByteOrder::BigEndian => 4u16.to_be_bytes(),
        };
        let mut bytes = vec![
            X_DRI3_MAJOR_OPCODE,
            X_DRI3_SET_DRM_DEVICE_IN_USE_MINOR_OPCODE,
        ];
        bytes.extend(length);
        bytes.extend(u32bytes(WINDOW.local.raw() as u32));
        bytes.extend(u32bytes(DEVICE.major));
        bytes.extend(u32bytes(DEVICE.minor));
        let context = XWireClientContext {
            byte_order,
            namespace: NS,
            transaction: TransactionId::from_raw(1),
            resource_id_range: None,
        };
        assert_eq!(
            decode_x11_core_request(context, &bytes).unwrap(),
            XWireRequest::Dri3SetDrmDeviceInUse {
                window: WINDOW,
                major: DEVICE.major,
                minor: DEVICE.minor,
            }
        );
        bytes.pop();
        assert!(decode_x11_core_request(context, &bytes).is_err());
    }
}
