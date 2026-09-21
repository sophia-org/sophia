#[test]
fn xi_grab_device_installs_only_the_bounded_master_pointer_mask() {
    let namespace = NamespaceId::from_raw(44);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let result = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            1,
            XByteOrder::LittleEndian,
            X_INPUT_MAJOR_OPCODE,
        ),
        XWireRequest::XiGrabDevice {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            time: 0,
            cursor: None,
            device_id: 2,
            pointer_mode: 1,
            keyboard_mode: 1,
            owner_events: false,
            event_mask: vec![0x70],
        },
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        result.outputs.as_slice(),
        [XClientOutput::Reply(XClientReply::GrabStatus { status: 0, .. })]
    ));
    let grab = runtime
        .input_authority_mut()
        .pointer_grab(namespace)
        .unwrap();
    assert_eq!(grab.event_mask, 0);
    assert!(grab.selects_xi_event(4));
    assert!(grab.selects_xi_event(5));
    assert!(grab.selects_xi_event(6));
    assert!(!grab.selects_xi_event(7));
}

#[test]
fn x11_dispatch_advertises_randr_and_replies_to_query_version() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let query = decode_x11_core_request(
        context(namespace, 538, XByteOrder::LittleEndian),
        &query_extension_request(XByteOrder::LittleEndian, X_RANDR_EXTENSION_NAME),
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 98),
        query,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][8], 1);
    assert_eq!(encoded[0][9], X_RANDR_MAJOR_OPCODE);

    let version = decode_x11_core_request(
        context(namespace, 539, XByteOrder::LittleEndian),
        &randr_query_version_request(XByteOrder::LittleEndian, 1, 5),
    )
    .unwrap();
    assert_eq!(
        version,
        XWireRequest::RandrQueryVersion {
            major_version: 1,
            minor_version: 5,
        }
    );
    let version = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, X_RANDR_MAJOR_OPCODE),
        version,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = version.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), 1);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][12..16]), 5);

    let select = decode_x11_core_request(
        context(namespace, 540, XByteOrder::LittleEndian),
        &randr_select_input_request(XByteOrder::LittleEndian, X_SETUP_DEFAULT_ROOT, 0x000b),
    )
    .unwrap();
    assert_eq!(
        select,
        XWireRequest::RandrSelectInput {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            enable: 0x000b,
        }
    );
    let select = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, X_RANDR_MAJOR_OPCODE),
        select,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(select.outputs.is_empty());

    let primary = decode_x11_core_request(
        context(namespace, 541, XByteOrder::LittleEndian),
        &randr_window_request(
            XByteOrder::LittleEndian,
            X_RANDR_GET_OUTPUT_PRIMARY_MINOR_OPCODE,
            X_SETUP_DEFAULT_ROOT,
        ),
    )
    .unwrap();
    assert_eq!(
        primary,
        XWireRequest::RandrGetOutputPrimary {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        }
    );
    let primary = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, X_RANDR_MAJOR_OPCODE),
        primary,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = primary.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]),
        0x2000_0001
    );

    let mut get_providers_request = vec![
        X_RANDR_MAJOR_OPCODE,
        X_RANDR_GET_PROVIDERS_MINOR_OPCODE,
        2,
        0,
    ];
    get_providers_request.extend_from_slice(&X_SETUP_DEFAULT_ROOT.to_le_bytes());
    let get_providers = decode_x11_core_request(
        context(namespace, 542, XByteOrder::LittleEndian),
        &get_providers_request,
    )
    .unwrap();
    assert_eq!(
        get_providers,
        XWireRequest::RandrGetProviders {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        }
    );
    let get_providers = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, X_RANDR_MAJOR_OPCODE),
        get_providers,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = get_providers.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][12..14]), 0);

    let monitors = decode_x11_core_request(
        context(namespace, 542, XByteOrder::LittleEndian),
        &randr_get_monitors_request(XByteOrder::LittleEndian, X_SETUP_DEFAULT_ROOT, true),
    )
    .unwrap();
    assert_eq!(
        monitors,
        XWireRequest::RandrGetMonitors {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            get_active: true,
        }
    );
    let monitors = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, X_RANDR_MAJOR_OPCODE),
        monitors,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = monitors.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][12..16]), 1);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][16..20]), 1);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 7);
    assert_eq!(encoded[0][36], 1, "the deterministic monitor is primary");
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][38..40]), 1);
    assert_eq!(
        read_u16(XByteOrder::LittleEndian, &encoded[0][44..46]),
        1280
    );
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][46..48]), 720);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[0][56..60]),
        0x2000_0001
    );
}

#[test]
fn randr_get_panning_reports_disabled_and_rejects_unknown_crtcs() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    for (sequence, crtc, expected_code) in [
        (1, 0x1000_0001, 1),
        (2, 0x1fff_ffff, 0),
    ] {
        let request = randr_crtc_request(
            XByteOrder::LittleEndian,
            X_RANDR_GET_PANNING_MINOR_OPCODE,
            crtc,
        );
        let request = decode_x11_core_request(
            context(
                namespace,
                542 + u64::from(sequence),
                XByteOrder::LittleEndian,
            ),
            &request,
        )
        .unwrap();
        assert_eq!(request, XWireRequest::RandrGetPanning { crtc });

        let encoded = dispatch_x11_wire_request(
            dispatch_context(
                namespace,
                sequence,
                XByteOrder::LittleEndian,
                X_RANDR_MAJOR_OPCODE,
            ),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        )
        .encoded_outputs(XByteOrder::LittleEndian);
        assert_eq!(encoded[0][0], expected_code);
        if expected_code == 1 {
            assert_eq!(encoded[0].len(), 36);
            assert_eq!(encoded[0][1], 0, "panning status is Success");
            assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 1);
            assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), 1);
            assert!(encoded[0][12..].iter().all(|byte| *byte == 0));
        } else {
            assert_eq!(encoded[0][1], 2, "unknown CRTC is BadValue");
            assert_eq!(
                read_u16(XByteOrder::LittleEndian, &encoded[0][8..10]),
                u16::from(X_RANDR_GET_PANNING_MINOR_OPCODE)
            );
            assert_eq!(encoded[0][10], X_RANDR_MAJOR_OPCODE);
        }
    }
}

#[test]
fn randr_get_crtc_transform_reports_bounded_identity_transform() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let request = randr_crtc_request(
        XByteOrder::LittleEndian,
        X_RANDR_GET_CRTC_TRANSFORM_MINOR_OPCODE,
        0x1000_0001,
    );
    let request = decode_x11_core_request(
        context(namespace, 545, XByteOrder::LittleEndian),
        &request,
    )
    .unwrap();
    assert_eq!(
        request,
        XWireRequest::RandrGetCrtcTransform {
            crtc: 0x1000_0001
        }
    );

    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, X_RANDR_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0].len(), 96);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 16);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), 1 << 16);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][24..28]), 1 << 16);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][40..44]), 1 << 16);
    assert_eq!(encoded[0][44], 0, "arbitrary transforms are unavailable");
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][48..52]), 1 << 16);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][64..68]), 1 << 16);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][80..84]), 1 << 16);
    assert!(encoded[0][84..].iter().all(|byte| *byte == 0));
}

#[test]
fn randr_get_crtc_gamma_matches_the_advertised_zero_length_ramp() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let request = randr_crtc_request(
        XByteOrder::LittleEndian,
        X_RANDR_GET_CRTC_GAMMA_MINOR_OPCODE,
        0x1000_0001,
    );
    let request = decode_x11_core_request(
        context(namespace, 546, XByteOrder::LittleEndian),
        &request,
    )
    .unwrap();
    assert_eq!(
        request,
        XWireRequest::RandrGetCrtcGamma {
            crtc: 0x1000_0001
        }
    );

    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, X_RANDR_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0].len(), 32);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 0);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][8..10]), 0);
}

#[test]
fn randr_output_property_returns_bounded_empty_edid_fallback() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let edid = atoms.intern("EDID", false).unwrap().unwrap();
    let request = decode_x11_core_request(
        context(namespace, 543, XByteOrder::LittleEndian),
        &randr_get_output_property_request(XByteOrder::LittleEndian, 0x2000_0001, edid, 128),
    )
    .unwrap();
    assert!(matches!(
        request,
        XWireRequest::RandrGetOutputProperty {
            output: 0x2000_0001,
            property,
            long_length: 128,
            ..
        } if property == edid
    ));
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 6, XByteOrder::LittleEndian, X_RANDR_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0].len(), 32);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][1], 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][12..16]), 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][16..20]), 0);
}

#[test]
fn randr_conventional_output_properties_are_valid_across_two_outputs() {
    let namespace = NamespaceId::from_raw(45);
    let topology = OutputTopologySnapshot {
        generation: 1,
        primary: OutputId::from_raw(1),
        outputs: vec![
            OutputTopologyEntry {
                output: OutputId::from_raw(1),
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 1280,
                    height: 720,
                },
                pixel_size: Size {
                    width: 1280,
                    height: 720,
                },
                scale: 1,
                refresh_millihz: 60_000,
                timing: None,
            },
            OutputTopologyEntry {
                output: OutputId::from_raw(2),
                logical: Rect {
                    x: 1280,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                pixel_size: Size {
                    width: 1920,
                    height: 1080,
                },
                scale: 1,
                refresh_millihz: 60_000,
                timing: None,
            },
        ],
    };
    let mut runtime = XAuthorityRuntime::with_output_topology(topology).unwrap();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let edid = atoms.atom(X_ATOM_NAME_RANDR_EDID).unwrap();
    let non_desktop = atoms.atom(X_ATOM_NAME_RANDR_NON_DESKTOP).unwrap();

    for (sequence, output) in [0x2000_0001, 0x2000_0002].into_iter().enumerate() {
        let edid_request = decode_x11_core_request(
            context(namespace, 600 + u64::try_from(sequence).unwrap(), XByteOrder::LittleEndian),
            &randr_get_output_property_request(
                XByteOrder::LittleEndian,
                output,
                edid,
                128,
            ),
        )
        .unwrap();
        let edid_result = dispatch_x11_wire_request(
            dispatch_context(
                namespace,
                u16::try_from(sequence + 1).unwrap(),
                XByteOrder::LittleEndian,
                X_RANDR_MAJOR_OPCODE,
            ),
            edid_request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        let encoded = edid_result.encoded_outputs(XByteOrder::LittleEndian);
        assert_eq!(encoded[0][0], 1);
        assert_eq!(encoded[0][1], 0);
        assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), 0);

        let non_desktop_request = decode_x11_core_request(
            context(namespace, 610 + u64::try_from(sequence).unwrap(), XByteOrder::LittleEndian),
            &randr_get_output_property_request(
                XByteOrder::LittleEndian,
                output,
                non_desktop,
                1,
            ),
        )
        .unwrap();
        let non_desktop_result = dispatch_x11_wire_request(
            dispatch_context(
                namespace,
                u16::try_from(sequence + 3).unwrap(),
                XByteOrder::LittleEndian,
                X_RANDR_MAJOR_OPCODE,
            ),
            non_desktop_request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        let encoded = non_desktop_result.encoded_outputs(XByteOrder::LittleEndian);
        assert_eq!(encoded[0][0], 1);
        assert_eq!(encoded[0][1], 32);
        assert_eq!(
            read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]),
            X_ATOM_CARDINAL
        );
        assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][16..20]), 1);
        assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][32..36]), 0);
    }

    let invalid_atom = decode_x11_core_request(
        context(namespace, 620, XByteOrder::LittleEndian),
        &randr_get_output_property_request(
            XByteOrder::LittleEndian,
            0x2000_0001,
            0xffff_fffe,
            1,
        ),
    )
    .unwrap();
    let invalid_atom = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            5,
            XByteOrder::LittleEndian,
            X_RANDR_MAJOR_OPCODE,
        ),
        invalid_atom,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(invalid_atom[0][0], 0);
    assert_eq!(invalid_atom[0][1], 5, "invalid property atom is BadAtom");

    let invalid_output = decode_x11_core_request(
        context(namespace, 621, XByteOrder::LittleEndian),
        &randr_get_output_property_request(
            XByteOrder::LittleEndian,
            0x2fff_ffff,
            edid,
            1,
        ),
    )
    .unwrap();
    let invalid_output = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            6,
            XByteOrder::LittleEndian,
            X_RANDR_MAJOR_OPCODE,
        ),
        invalid_output,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(invalid_output[0][0], 0);
    assert_eq!(invalid_output[0][1], 2, "invalid output is BadValue");
}

#[test]
fn xfixes_regions_support_create_set_and_destroy_lifecycle() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let region = 0x220100;
    let rectangles = [Rect {
        x: 0,
        y: 0,
        width: 310,
        height: 257,
    }];

    for (sequence, request) in [
        xfixes_create_region_request(XByteOrder::LittleEndian, region, &[]),
        xfixes_set_region_request(XByteOrder::LittleEndian, region, &rectangles),
    ]
    .into_iter()
    .enumerate()
    {
        let request = decode_x11_core_request(
            context(namespace, 540 + sequence as u64, XByteOrder::LittleEndian),
            &request,
        )
        .unwrap();
        if sequence == 1 {
            assert!(matches!(
                request,
                XWireRequest::XfixesSetRegion {
                    rectangles: ref decoded,
                    ..
                } if decoded == &rectangles
            ));
        }
        let result = dispatch_x11_wire_request(
            dispatch_context(
                namespace,
                5 + sequence as u16,
                XByteOrder::LittleEndian,
                X_XFIXES_MAJOR_OPCODE,
            ),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(result.outputs.is_empty());
    }

    let region_id = XResourceId::new(u64::from(region), 1);
    assert_eq!(
        runtime.validate_xfixes_region_access(namespace, region_id),
        Ok(())
    );
    let destroy = XWireRequest::XfixesDestroyRegion { region: region_id };
    let result = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            7,
            XByteOrder::LittleEndian,
            X_XFIXES_MAJOR_OPCODE,
        ),
        destroy,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(result.outputs.is_empty());
    assert_eq!(
        runtime.validate_xfixes_region_access(namespace, region_id),
        Err(XAuthorityRuntimeError::UnknownResource)
    );
}

/// Watching a selection is scoped to a window, not an action upon one, so the
/// root is the ordinary argument: every toolkit calls
/// `XFixesSelectSelectionInput(dpy, DefaultRootWindow(dpy), CLIPBOARD, mask)`.
/// Refusing it produced a `BadWindow` storm that failed a physical session.
#[test]
fn root_scoped_requests_are_admitted_without_a_client_window() {
    let namespace = NamespaceId::from_raw(61);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let selection = decode_x11_core_request(
        context(namespace, 600, XByteOrder::LittleEndian),
        &xfixes_select_selection_input_request(
            XByteOrder::LittleEndian,
            X_SETUP_DEFAULT_ROOT,
            X_ATOM_PRIMARY,
            0b111,
        ),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            8,
            XByteOrder::LittleEndian,
            X_XFIXES_MAJOR_OPCODE,
        ),
        selection,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        result.outputs.is_empty(),
        "selection watching on the root must not error"
    );

    let present = decode_x11_core_request(
        context(namespace, 601, XByteOrder::LittleEndian),
        &present_select_input_request(
            XByteOrder::LittleEndian,
            0x220400,
            X_SETUP_DEFAULT_ROOT,
            0,
        ),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            9,
            XByteOrder::LittleEndian,
            X_PRESENT_MAJOR_OPCODE,
        ),
        present,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        result.outputs.is_empty(),
        "Present event selection on the root must not error"
    );

    // Setting the root cursor names the root for scope in the same way.
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 10, XByteOrder::LittleEndian, X_INPUT_MAJOR_OPCODE),
        XWireRequest::XiChangeCursor {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            cursor: None,
        },
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        result.outputs.is_empty(),
        "clearing the root cursor must not error"
    );
}

/// Present refusals name the request that produced them.
///
/// The equivalent XFIXES assertion below has existed for some time; Present
/// had none, and that is how a live session came to report nine refusals under
/// `major=138 minor=0`. Minor 0 is `QueryVersion`, which takes no drawable and
/// cannot return `BadWindow`, so the evidence named a request that could not
/// have failed and the real one stayed hidden.
#[test]
fn present_event_selection_refuses_an_unknown_window_by_name() {
    let namespace = NamespaceId::from_raw(63);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let unknown = 0x22_0998;

    let request = decode_x11_core_request(
        context(namespace, 603, XByteOrder::LittleEndian),
        &present_select_input_request(XByteOrder::LittleEndian, 0x220401, unknown, 0),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            11,
            XByteOrder::LittleEndian,
            X_PRESENT_MAJOR_OPCODE,
        ),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        matches!(
            result.outputs.as_slice(),
            [XClientOutput::Error(XClientError {
                code: XErrorCode::BadWindow,
                resource_id,
                minor_code: 3,
                major_code: X_PRESENT_MAJOR_OPCODE,
                ..
            })] if *resource_id == unknown
        ),
        "{:?}",
        result.outputs
    );
}

/// The root is admitted; an id that is neither the root nor a client window is
/// still refused, and still names the request that refused it.
#[test]
fn selection_watching_still_refuses_an_unknown_window() {
    let namespace = NamespaceId::from_raw(62);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let unknown = 0x22_0999;

    let request = decode_x11_core_request(
        context(namespace, 602, XByteOrder::LittleEndian),
        &xfixes_select_selection_input_request(
            XByteOrder::LittleEndian,
            unknown,
            X_ATOM_PRIMARY,
            0b111,
        ),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            10,
            XByteOrder::LittleEndian,
            X_XFIXES_MAJOR_OPCODE,
        ),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        result.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadWindow,
            resource_id,
            minor_code: 2,
            major_code: X_XFIXES_MAJOR_OPCODE,
            ..
        })] if *resource_id == unknown
    ));
}

#[test]
fn xfixes_selection_subscription_accepts_known_window_atom_and_mask() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let window = 0x220101;
    let create = decode_x11_core_request(
        context(namespace, 543, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 1, 1),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 6, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let request = decode_x11_core_request(
        context(namespace, 544, XByteOrder::LittleEndian),
        &xfixes_select_selection_input_request(
            XByteOrder::LittleEndian,
            window,
            X_ATOM_PRIMARY,
            0b111,
        ),
    )
    .unwrap();
    assert!(matches!(
        request,
        XWireRequest::XfixesSelectSelectionInput {
            selection: X_ATOM_PRIMARY,
            event_mask: 0b111,
            ..
        }
    ));
    let result = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            7,
            XByteOrder::LittleEndian,
            X_XFIXES_MAJOR_OPCODE,
        ),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(result.outputs.is_empty());
}

#[test]
fn x11_dispatch_advertises_probe_backed_xkeyboard_extension() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let query = decode_x11_core_request(
        context(namespace, 545, XByteOrder::LittleEndian),
        &query_extension_request(XByteOrder::LittleEndian, X_KEYBOARD_EXTENSION_NAME),
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 98),
        query,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][8], 1);
    assert_eq!(encoded[0][9], X_KEYBOARD_MAJOR_OPCODE);
    assert_eq!(encoded[0][10], X_KEYBOARD_FIRST_EVENT);

    let use_extension = decode_x11_core_request(
        context(namespace, 546, XByteOrder::LittleEndian),
        &xkb_use_extension_request(XByteOrder::LittleEndian, 1, 0),
    )
    .unwrap();
    assert_eq!(
        use_extension,
        XWireRequest::XkbUseExtension {
            wanted_major: 1,
            wanted_minor: 0,
        }
    );
    let use_extension = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            2,
            XByteOrder::LittleEndian,
            X_KEYBOARD_MAJOR_OPCODE,
        ),
        use_extension,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = use_extension.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][1], 1);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][8..10]), 1);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][10..12]), 0);
}

#[test]
fn extension_event_ranges_do_not_replace_core_or_each_other() {
    let ranges = [
        ("RANDR", X_RANDR_FIRST_EVENT, 2_u8),
        ("XFIXES", X_XFIXES_FIRST_EVENT, 2),
        ("SYNC", X_SYNC_FIRST_EVENT, 2),
        ("XKEYBOARD", X_KEYBOARD_FIRST_EVENT, 1),
        ("GLX", X_GLX_FIRST_EVENT, 17),
        ("XInputExtension", X_INPUT_FIRST_EVENT, 17),
        ("MIT-SHM", X_MIT_SHM_FIRST_EVENT, 1),
    ];
    let mut owners = std::collections::BTreeMap::new();
    for (name, first, count) in ranges {
        assert!(
            first > 35,
            "{name} event base {first} collides with core X11 events"
        );
        for event_type in first..first + count {
            assert!(
                owners.insert(event_type, name).is_none(),
                "{name} event type {event_type} overlaps another extension"
            );
        }

        let namespace = NamespaceId::from_raw(46);
        let query = decode_x11_core_request(
            context(namespace, u64::from(first), XByteOrder::LittleEndian),
            &query_extension_request(XByteOrder::LittleEndian, name),
        )
        .unwrap();
        let encoded = dispatch_x11_wire_request(
            dispatch_context(namespace, 1, XByteOrder::LittleEndian, 98),
            query,
            &mut XAuthorityRuntime::new(),
            &mut XAtomTable::new(),
            &mut XPropertyTable::new(),
        )
        .encoded_outputs(XByteOrder::LittleEndian);
        assert_eq!(
            encoded[0][10], first,
            "{name} did not advertise its allocated event base"
        );
    }
}

#[test]
fn x11_dispatch_advertises_non_core_glx_event_base() {
    let namespace = NamespaceId::from_raw(46);
    let query = decode_x11_core_request(
        context(namespace, 547, XByteOrder::LittleEndian),
        &query_extension_request(XByteOrder::LittleEndian, X_GLX_EXTENSION_NAME),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 98),
        query,
        &mut XAuthorityRuntime::new(),
        &mut XAtomTable::new(),
        &mut XPropertyTable::new(),
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][9], X_GLX_MAJOR_OPCODE);
    assert_eq!(encoded[0][10], X_GLX_FIRST_EVENT);
}

#[test]
fn xkb_state_names_and_state_subscription_use_standard_wire_layouts() {
    let namespace = NamespaceId::from_raw(45);
    let order = XByteOrder::LittleEndian;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let get_state = decode_x11_core_request(
        context(namespace, 1, order),
        &[
            X_KEYBOARD_MAJOR_OPCODE,
            X_KEYBOARD_GET_STATE_MINOR_OPCODE,
            2,
            0,
            3,
            0,
            0,
            0,
        ],
    )
    .unwrap();
    assert_eq!(get_state, XWireRequest::XkbGetState);
    let state = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, order, X_KEYBOARD_MAJOR_OPCODE),
        get_state,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(order);
    assert_eq!(state[0].len(), 32);
    assert_eq!(state[0][1], 3);

    let names = decode_x11_core_request(
        context(namespace, 2, order),
        &[
            X_KEYBOARD_MAJOR_OPCODE,
            X_KEYBOARD_GET_NAMES_MINOR_OPCODE,
            3,
            0,
            3,
            0,
            0,
            0,
            0x3f,
            0,
            0,
            0,
        ],
    )
    .unwrap();
    assert_eq!(names, XWireRequest::XkbGetNames { which: 0x3f });
    let names = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, order, X_KEYBOARD_MAJOR_OPCODE),
        names,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(order);
    assert_eq!(read_u32(order, &names[0][8..12]), 0x3f);
    assert_eq!(names[0].len(), 56);
    assert_eq!(names[0][12], 8);
    assert_eq!(names[0][13], u8::MAX);

    let select = decode_x11_core_request(
        context(namespace, 3, order),
        &[
            X_KEYBOARD_MAJOR_OPCODE,
            X_KEYBOARD_SELECT_EVENTS_MINOR_OPCODE,
            5,
            0,
            3,
            0,
            4,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            1,
            0,
            1,
            0,
        ],
    )
    .unwrap();
    assert_eq!(
        select,
        XWireRequest::XkbSelectEvents {
            affect_which: 4,
            clear: 0,
            select_all: 0,
            state_details: Some((1, 1)),
        }
    );

    let notify = encode_x_client_event(
        order,
        XClientEvent::XkbStateNotify {
            sequence: 7,
            time: 11,
            modifiers: 1,
            changed: 1,
            keycode: 50,
            event_type: 2,
        },
    );
    assert_eq!(notify[0], X_KEYBOARD_FIRST_EVENT);
    assert_eq!(notify[1], 2);
    assert_eq!(read_u16(order, &notify[24..26]), 1);
    assert_eq!(notify[26], 50);
}

#[test]
fn xge_and_xi2_report_versioned_master_device_classes() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let query = decode_x11_core_request(
        context(namespace, 1, XByteOrder::LittleEndian),
        &query_extension_request(XByteOrder::LittleEndian, X_GENERIC_EVENT_EXTENSION_NAME),
    )
    .unwrap();
    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 98),
        query,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][8], 1);
    assert_eq!(encoded[0][9], X_GENERIC_EVENT_MAJOR_OPCODE);

    let version = decode_x11_core_request(
        context(namespace, 2, XByteOrder::LittleEndian),
        &[X_GENERIC_EVENT_MAJOR_OPCODE, 0, 2, 0, 1, 0, 0, 0],
    )
    .unwrap();
    assert_eq!(
        version,
        XWireRequest::GeQueryVersion {
            major_version: 1,
            minor_version: 0
        }
    );
    let encoded = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            2,
            XByteOrder::LittleEndian,
            X_GENERIC_EVENT_MAJOR_OPCODE,
        ),
        version,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][8..10]), 1);

    let xi_version = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, X_INPUT_MAJOR_OPCODE),
        XWireRequest::XiQueryVersion {
            major_version: 2,
            minor_version: 3,
        },
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(
        read_u16(XByteOrder::LittleEndian, &xi_version[0][8..10]),
        2
    );
    assert_eq!(
        read_u16(XByteOrder::LittleEndian, &xi_version[0][10..12]),
        1
    );

    let devices = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, X_INPUT_MAJOR_OPCODE),
        XWireRequest::XiQueryDevice { device_id: 0 },
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &devices[0][8..10]), 3);
    let pointer_class_count = read_u16(XByteOrder::LittleEndian, &devices[0][38..40]);
    assert_eq!(pointer_class_count, 7);
    let pointer_name_len = usize::from(read_u16(
        XByteOrder::LittleEndian,
        &devices[0][40..42],
    ));
    let mut class_offset = 44 + pointer_name_len.next_multiple_of(4);
    let mut valuators = Vec::new();
    let mut scrolls = Vec::new();
    for _ in 0..pointer_class_count {
        let class_type = read_u16(
            XByteOrder::LittleEndian,
            &devices[0][class_offset..class_offset + 2],
        );
        let class_len = usize::from(read_u16(
            XByteOrder::LittleEndian,
            &devices[0][class_offset + 2..class_offset + 4],
        )) * 4;
        match class_type {
            2 => valuators.push((
                read_u16(
                    XByteOrder::LittleEndian,
                    &devices[0][class_offset + 6..class_offset + 8],
                ),
                (i64::from(read_u32(
                    XByteOrder::LittleEndian,
                    &devices[0][class_offset + 12..class_offset + 16],
                ) as i32) << 32) | i64::from(read_u32(
                    XByteOrder::LittleEndian,
                    &devices[0][class_offset + 16..class_offset + 20],
                )),
                (i64::from(read_u32(
                    XByteOrder::LittleEndian,
                    &devices[0][class_offset + 20..class_offset + 24],
                ) as i32) << 32) | i64::from(read_u32(
                    XByteOrder::LittleEndian,
                    &devices[0][class_offset + 24..class_offset + 28],
                )),
                (i64::from(read_u32(
                    XByteOrder::LittleEndian,
                    &devices[0][class_offset + 28..class_offset + 32],
                ) as i32) << 32) | i64::from(read_u32(
                    XByteOrder::LittleEndian,
                    &devices[0][class_offset + 32..class_offset + 36],
                )),
            )),
            3 => scrolls.push((
                read_u16(
                    XByteOrder::LittleEndian,
                    &devices[0][class_offset + 6..class_offset + 8],
                ),
                read_u16(
                    XByteOrder::LittleEndian,
                    &devices[0][class_offset + 8..class_offset + 10],
                ),
            )),
            _ => {}
        }
        class_offset += class_len;
    }
    assert_eq!(
        valuators,
        vec![
            (0, 0, i64::from(u16::MAX) << 32, 0),
            (1, 0, i64::from(u16::MAX) << 32, 0),
            (2, 0, 0, 0),
            (3, 0, 0, 0),
        ]
    );
    assert_eq!(scrolls, vec![(2, 2), (3, 1)]);
    assert!(devices[0].len() > 128);
}

#[test]
fn xi_query_pointer_encodes_coordinates_buttons_and_modifiers() {
    let reply = encode_x_client_output(
        XByteOrder::LittleEndian,
        XClientOutput::Reply(XClientReply::XiQueryPointer {
            sequence: 9,
            root: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            child: XResourceId::new(0x220031, 1),
            root_x: 320,
            root_y: 240,
            win_x: -12,
            win_y: 18,
            buttons: (1 << 1) | (1 << 3),
            modifiers: 5,
        }),
    );

    assert_eq!(reply.len(), 60);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &reply[12..16]), 0x220031);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &reply[16..20]), 320 << 16);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reply[24..28]),
        (-12_i32 << 16) as u32
    );
    assert_eq!(read_u16(XByteOrder::LittleEndian, &reply[34..36]), 1);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &reply[48..52]), 5);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &reply[56..60]), 10);
}

#[test]
fn xkb_get_map_encodes_schema_aligned_types_symbols_and_modifier_map() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let result = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            4,
            XByteOrder::LittleEndian,
            X_KEYBOARD_MAJOR_OPCODE,
        ),
        XWireRequest::XkbGetMap {
            full: 0x47,
            partial: 0,
        },
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    let reply = &encoded[0];
    assert_eq!(&reply[8..10], &[0, 0]);
    assert_eq!(reply[10], 8);
    assert_eq!(reply[11], u8::MAX);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &reply[12..14]), 0x47);
    assert_eq!(&reply[14..18], &[0, 4, 4, 8]);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &reply[18..20]), 496);
    assert_eq!(reply[20], 248);
    assert_eq!(&reply[31..34], &[8, 248, 10]);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reply[4..8]) as usize,
        (reply.len() - 32) / 4
    );
    assert_eq!(&reply[40..48], &[1, 1, 0, 0, 2, 1, 0, 0]);
    assert_eq!(&reply[104..112], &[0, 0, 0, 0, 1, 2, 2, 0]);
}

#[test]
fn xkb_state_uses_deterministic_rmlvo_and_tracks_effective_modifiers() {
    let mut keyboard = XkbKeyboardState::new(&XkbRmlvoConfig::default()).unwrap();
    assert_eq!(keyboard.map_evdev_key(42, true), Some((50, 0)));
    assert_eq!(keyboard.map_evdev_key(30, true), Some((38, 1)));
    assert_eq!(keyboard.map_evdev_key(30, false), Some((38, 1)));
    assert_eq!(keyboard.map_evdev_key(42, false), Some((50, 1)));
    assert_eq!(keyboard.modifier_mask(), 0);
}

#[test]
fn xkb_snapshot_drives_core_and_xkb_maps_from_the_same_rmlvo() {
    let us = XkbKeymapSnapshot::new(&XkbRmlvoConfig::default()).unwrap();
    let de_config = XkbRmlvoConfig {
        layout: "de".to_owned(),
        ..XkbRmlvoConfig::default()
    };
    let de = XkbKeymapSnapshot::new(&de_config).unwrap();

    assert_eq!(us.config().layout, "us");
    assert_eq!(de.config().layout, "de");
    assert_eq!(us.core_mapping(8, 248), us.xkb_keysyms().concat());
    assert_eq!(de.core_mapping(8, 248), de.xkb_keysyms().concat());
    assert_ne!(us.core_mapping(29, 1), de.core_mapping(29, 1));
}

#[test]
fn xkb_rmlvo_validation_rejects_empty_and_unbounded_configuration() {
    let mut empty = XkbRmlvoConfig::default();
    empty.layout.clear();
    assert_eq!(
        XkbKeyboardState::new(&empty).unwrap_err(),
        XkbKeyboardError::InvalidConfiguration
    );

    let unbounded = XkbRmlvoConfig {
        options: "x".repeat(XKB_RMLVO_FIELD_MAX_BYTES + 1),
        ..XkbRmlvoConfig::default()
    };
    assert_eq!(
        XkbKeyboardState::new(&unbounded).unwrap_err(),
        XkbKeyboardError::InvalidConfiguration
    );
}

/// MIT-SHM 1.2 refuses what it cannot honour, at the request that asked.
///
/// `ShmQueryVersion` advertises 1.2, so these two opcodes have to exist or the
/// advertisement is a lie -- which is exactly what it was until they did, and
/// a Qt shell paid for believing it. The socket round trip is proven by
/// `x-authority-shm-fd-smoke`; what is checked here is the refusals, which a
/// well-behaved client never reaches.
#[test]
fn shm_descriptor_segments_refuse_a_bad_size_and_a_used_name() {
    let namespace = NamespaceId::from_raw(70);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    // A CARD32 can name four gigabytes; the adapter will not map it.
    let oversize = 0x22_0701;
    let request = decode_x11_core_request(
        context(namespace, 700, XByteOrder::LittleEndian),
        &mit_shm_create_segment_request(XByteOrder::LittleEndian, oversize, u32::MAX, false),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 20, XByteOrder::LittleEndian, X_MIT_SHM_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        matches!(
            result.outputs.as_slice(),
            [XClientOutput::Error(XClientError {
                minor_code: 7,
                major_code: X_MIT_SHM_MAJOR_OPCODE,
                ..
            })]
        ),
        "{:?}",
        result.outputs
    );

    // A size it will map is accepted, and the reply is what carries the
    // descriptor out.
    let segment = 0x22_0702;
    let request = decode_x11_core_request(
        context(namespace, 701, XByteOrder::LittleEndian),
        &mit_shm_create_segment_request(XByteOrder::LittleEndian, segment, 4096, false),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 21, XByteOrder::LittleEndian, X_MIT_SHM_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        matches!(
            result.outputs.as_slice(),
            [XClientOutput::Reply(XClientReply::ShmCreateSegment { .. })]
        ),
        "{:?}",
        result.outputs
    );

    // Naming it again is the client's mistake, and it is told which request
    // made it rather than being left to guess.
    let request = decode_x11_core_request(
        context(namespace, 702, XByteOrder::LittleEndian),
        &mit_shm_attach_fd_request(XByteOrder::LittleEndian, segment, false),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 22, XByteOrder::LittleEndian, X_MIT_SHM_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        matches!(
            result.outputs.as_slice(),
            [XClientOutput::Error(XClientError {
                code: XErrorCode::BadIdChoice,
                minor_code: 6,
                major_code: X_MIT_SHM_MAJOR_OPCODE,
                ..
            })]
        ),
        "{:?}",
        result.outputs
    );
}

/// A 2560x1440 mode at a nominal 120 Hz, with the blanking a real panel has.
///
/// It does not run at 120: `497'751 kHz` over `2720 * 1525` pixels is
/// 119.997 Hz. That gap is the reason this extension is worth answering, and
/// the reason the nominal rate is kept beside the measured one rather than
/// replaced by it.
fn dp1_timing() -> sophia_protocol::OutputModeTiming {
    sophia_protocol::OutputModeTiming {
        clock_khz: 497_751,
        hdisplay: 2560,
        hsync_start: 2608,
        hsync_end: 2640,
        htotal: 2720,
        hskew: 0,
        vdisplay: 1440,
        vsync_start: 1443,
        vsync_end: 1448,
        vtotal: 1525,
        flags: 0,
    }
}

fn vidmode_topology(timing: Option<sophia_protocol::OutputModeTiming>) -> OutputTopologySnapshot {
    OutputTopologySnapshot {
        generation: 1,
        primary: OutputId::from_raw(1),
        outputs: vec![OutputTopologyEntry {
            output: OutputId::from_raw(1),
            logical: Rect {
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
            },
            pixel_size: Size {
                width: 2560,
                height: 1440,
            },
            scale: 1,
            // Nominal, as a profile writes it and the matcher compares it.
            refresh_millihz: 120_000,
            timing,
        }],
    }
}

/// The modeline reported is the one the display is running.
///
/// Mesa implements `glXGetMscRateOML` by dividing this clock by these totals,
/// so the arithmetic below is what a GL client ends up believing about the
/// refresh rate. Brave asked for it once per frame and was told the extension
/// did not exist.
#[test]
fn vidmode_reports_the_measured_modeline_not_the_nominal_rate() {
    let namespace = NamespaceId::from_raw(80);
    let mut runtime = XAuthorityRuntime::with_output_topology(vidmode_topology(Some(dp1_timing())))
        .unwrap();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let result = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            30,
            XByteOrder::LittleEndian,
            X_XF86_VIDMODE_MAJOR_OPCODE,
        ),
        XWireRequest::XF86VidModeGetModeLine { screen: 0 },
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let timing = match result.outputs.as_slice() {
        [XClientOutput::Reply(XClientReply::XF86VidModeGetModeLine { timing, .. })] => *timing,
        other => panic!("{other:?}"),
    };
    assert_eq!(timing, dp1_timing());

    // What Mesa computes, and the point of the whole exercise: the measured
    // rate is not the nominal one, and only this reply can say so.
    let measured = timing.measured_refresh_millihz().unwrap();
    assert_eq!(measured, 119_997);
    assert_ne!(measured, 120_000);
}

/// An output with no measured timing is refused, not answered with a guess.
///
/// A client given invented timings computes a refresh rate from them and
/// believes it. One given an error falls back to its own default and knows
/// that it did.
#[test]
fn vidmode_refuses_an_output_whose_timing_was_never_measured() {
    let namespace = NamespaceId::from_raw(81);
    let mut runtime = XAuthorityRuntime::with_output_topology(vidmode_topology(None)).unwrap();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    for screen in [0u16, 3] {
        let result = dispatch_x11_wire_request(
            dispatch_context(
                namespace,
                31,
                XByteOrder::LittleEndian,
                X_XF86_VIDMODE_MAJOR_OPCODE,
            ),
            XWireRequest::XF86VidModeGetModeLine { screen },
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(
            matches!(
                result.outputs.as_slice(),
                [XClientOutput::Error(XClientError {
                    code: XErrorCode::BadValue,
                    minor_code: 1,
                    major_code: X_XF86_VIDMODE_MAJOR_OPCODE,
                    ..
                })]
            ),
            "screen {screen}: {:?}",
            result.outputs
        );
    }
}

/// Version two is what makes `libXxf86vm` read the modern reply shape, and
/// `SetClientVersion` is what it sends immediately afterwards.
///
/// Refusing that second request would end the exchange one step after the
/// first had just succeeded. Everything else in the extension is declined by
/// name, because Sophia owns modesetting and a client must not reach for it
/// through a legacy extension.
#[test]
fn vidmode_answers_the_two_requests_mesa_needs_and_declines_the_rest() {
    let namespace = NamespaceId::from_raw(82);
    let mut runtime = XAuthorityRuntime::with_output_topology(vidmode_topology(Some(dp1_timing())))
        .unwrap();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let mut dispatch = |request| {
        dispatch_x11_wire_request(
            dispatch_context(
                namespace,
                32,
                XByteOrder::LittleEndian,
                X_XF86_VIDMODE_MAJOR_OPCODE,
            ),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        )
    };

    let version = dispatch(XWireRequest::XF86VidModeQueryVersion);
    assert!(
        matches!(
            version.outputs.as_slice(),
            [XClientOutput::Reply(XClientReply::XF86VidModeQueryVersion {
                major_version: 2,
                ..
            })]
        ),
        "{:?}",
        version.outputs
    );

    let client_version = dispatch(XWireRequest::XF86VidModeSetClientVersion {
        major: 2,
        minor: 2,
    });
    assert!(
        client_version.outputs.is_empty(),
        "SetClientVersion must be accepted silently: {:?}",
        client_version.outputs
    );

    // SwitchToMode, as an example of the surface that stays closed. Version 2.2
    // defines it, so a server of that version has a dispatch entry for it and
    // owes BadImplementation; answering BadRequest would claim the request does
    // not exist at the version just negotiated.
    let refused = dispatch(XWireRequest::XF86VidModeUnimplemented { minor_opcode: 10 });
    assert!(
        matches!(
            refused.outputs.as_slice(),
            [XClientOutput::Error(XClientError {
                code: XErrorCode::BadImplementation,
                minor_code: 10,
                major_code: X_XF86_VIDMODE_MAJOR_OPCODE,
                ..
            })]
        ),
        "{:?}",
        refused.outputs
    );

    // Past the last minor 2.2 defines, where a genuine server of this version
    // had no entry at all.
    let unknown = dispatch(XWireRequest::XF86VidModeUnimplemented { minor_opcode: 200 });
    assert!(
        matches!(
            unknown.outputs.as_slice(),
            [XClientOutput::Error(XClientError {
                code: XErrorCode::BadRequest,
                minor_code: 200,
                major_code: X_XF86_VIDMODE_MAJOR_OPCODE,
                ..
            })]
        ),
        "{:?}",
        unknown.outputs
    );
}

/// XC-MISC answers, and its default answer is the honest one.
///
/// A client reaches this only after exhausting the identifiers it was given at
/// connection setup, which a browser left open for days eventually does. The
/// dispatch layer cannot see the range counter -- that belongs to the socket
/// layer -- so it answers "none available", and the socket layer replaces that
/// with a grant when it can.
///
/// A count of zero is a real protocol answer that clients handle by giving up
/// cleanly. Inventing a range instead would hand out identifiers belonging to
/// another client, which is worse than the exhaustion it was avoiding.
#[test]
fn xc_misc_defaults_to_reporting_no_identifiers_rather_than_inventing_some() {
    let namespace = NamespaceId::from_raw(85);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let mut dispatch = |request| {
        dispatch_x11_wire_request(
            dispatch_context(namespace, 40, XByteOrder::LittleEndian, X_XC_MISC_MAJOR_OPCODE),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        )
    };

    let version = dispatch(XWireRequest::XCMiscGetVersion { major: 1, minor: 1 });
    assert!(
        matches!(
            version.outputs.as_slice(),
            [XClientOutput::Reply(XClientReply::XCMiscGetVersion {
                major_version: 1,
                minor_version: 1,
                ..
            })]
        ),
        "{:?}",
        version.outputs
    );

    let range = dispatch(XWireRequest::XCMiscGetXIDRange);
    assert!(
        matches!(
            range.outputs.as_slice(),
            [XClientOutput::Reply(XClientReply::XCMiscGetXIDRange {
                start_id: 0,
                count: 0,
                ..
            })]
        ),
        "{:?}",
        range.outputs
    );

    // Asking for four billion identifiers must not produce four billion
    // words in memory before anything has looked at the number.
    let list = dispatch(XWireRequest::XCMiscGetXIDList { count: u32::MAX });
    match list.outputs.as_slice() {
        [XClientOutput::Reply(XClientReply::XCMiscGetXIDList { ids, .. })] => {
            assert!(ids.is_empty(), "{ids:?}");
        }
        other => panic!("{other:?}"),
    }
}

/// The RENDER handshake answers the lower version, and the formats it
/// reports are the visuals' formats.
///
/// A client binds a picture format to a visual and expects the bytes it drew
/// through core requests to mean the same thing through RENDER, so agreement
/// between the two tables is the whole reply.
#[test]
fn render_handshake_answers_the_lower_version_and_the_visuals_formats() {
    let namespace = NamespaceId::from_raw(86);
    let byte_order = XByteOrder::LittleEndian;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    // The version answered is the lower of the two.
    for (asked, answered) in [((0, 99), (0, 10)), ((0, 2), (0, 2)), ((1, 0), (0, 10))] {
        let request = decode_x11_core_request(
            context(namespace, 701, byte_order),
            &render_query_version_request(byte_order, asked.0, asked.1),
        )
        .unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, 2, byte_order, X_RENDER_MAJOR_OPCODE),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        let encoded = result.encoded_outputs(byte_order);
        assert_eq!(read_u32(byte_order, &encoded[0][8..12]), answered.0);
        assert_eq!(read_u32(byte_order, &encoded[0][12..16]), answered.1);
    }

    // The four formats, and their agreement with the setup visuals. A client
    // binds a format to a visual and expects core-drawn bytes to mean the
    // same thing through RENDER, so the shifts and masks here must
    // reconstruct exactly the channel masks the visual advertises.
    let request = decode_x11_core_request(
        context(namespace, 702, byte_order),
        &render_query_pict_formats_request(byte_order),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, byte_order, X_RENDER_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(byte_order);
    let reply = &encoded[0];
    assert_eq!(read_u32(byte_order, &reply[8..12]), 4, "format count");
    assert_eq!(read_u32(byte_order, &reply[12..16]), 1, "screen count");
    assert_eq!(read_u32(byte_order, &reply[16..20]), 2, "depth count");
    assert_eq!(read_u32(byte_order, &reply[20..24]), 2, "visual count");
    let channel = |offset: usize| -> u32 {
        u32::from(read_u16(byte_order, &reply[offset + 2..offset + 4]))
            << read_u16(byte_order, &reply[offset..offset + 2])
    };
    let mut formats = std::collections::BTreeMap::new();
    for index in 0..4 {
        let offset = 32 + index * 28;
        let id = read_u32(byte_order, &reply[offset..offset + 4]);
        let depth = reply[offset + 5];
        let (red, green, blue, alpha) = (
            channel(offset + 8),
            channel(offset + 12),
            channel(offset + 16),
            channel(offset + 20),
        );
        formats.insert(id, (depth, red, green, blue, alpha));
    }
    let argb_visual = x_true_color_visual(X_SETUP_ARGB_VISUAL).unwrap();
    assert_eq!(
        formats.get(&X_RENDER_FORMAT_ARGB32),
        Some(&(
            32,
            argb_visual.red_mask,
            argb_visual.green_mask,
            argb_visual.blue_mask,
            argb_visual.alpha_mask,
        ))
    );
    let default_visual = x_true_color_visual(X_SETUP_DEFAULT_VISUAL).unwrap();
    assert_eq!(
        formats.get(&X_RENDER_FORMAT_RGB24),
        Some(&(
            24,
            default_visual.red_mask,
            default_visual.green_mask,
            default_visual.blue_mask,
            0,
        ))
    );
    assert_eq!(formats.get(&X_RENDER_FORMAT_A8), Some(&(8, 0, 0, 0, 0xff)));
    assert_eq!(formats.get(&X_RENDER_FORMAT_A1), Some(&(1, 0, 0, 0, 0x1)));

    // The screen maps each visual-bearing depth to its format.
    let screen = 32 + 4 * 28;
    assert_eq!(read_u32(byte_order, &reply[screen..screen + 4]), 2);
    assert_eq!(
        read_u32(byte_order, &reply[screen + 4..screen + 8]),
        X_RENDER_FORMAT_RGB24,
        "fallback format"
    );
    let depth24 = screen + 8;
    assert_eq!(reply[depth24], 24);
    assert_eq!(read_u16(byte_order, &reply[depth24 + 2..depth24 + 4]), 1);
    assert_eq!(
        read_u32(byte_order, &reply[depth24 + 8..depth24 + 12]),
        X_SETUP_DEFAULT_VISUAL
    );
    assert_eq!(
        read_u32(byte_order, &reply[depth24 + 12..depth24 + 16]),
        X_RENDER_FORMAT_RGB24
    );
    let depth32 = depth24 + 16;
    assert_eq!(reply[depth32], 32);
    assert_eq!(
        read_u32(byte_order, &reply[depth32 + 8..depth32 + 12]),
        X_SETUP_ARGB_VISUAL
    );
    assert_eq!(
        read_u32(byte_order, &reply[depth32 + 12..depth32 + 16]),
        X_RENDER_FORMAT_ARGB32
    );
}

/// RENDER refusals are two-tier, and each names the minor it declines.
///
/// A minor defined within the advertised version answers BadImplementation:
/// the request exists here and is not offered, which is also what Xorg
/// answers for the five it never wrote. A minor beyond the advertised
/// version answers BadRequest, because a genuine server of that version had
/// no dispatch entry for it at all. The split is what lets a client's
/// version-gated fallback logic work unmodified.
#[test]
fn render_refusals_split_between_not_offered_and_not_that_version() {
    let namespace = NamespaceId::from_raw(87);
    let byte_order = XByteOrder::LittleEndian;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let mut refusal_for = |minor: u8| -> (XErrorCode, u16) {
        let request = decode_x11_core_request(
            context(namespace, 710, byte_order),
            &render_minor_request(byte_order, minor),
        )
        .unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, 4, byte_order, X_RENDER_MAJOR_OPCODE),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        match result.outputs.as_slice() {
            [XClientOutput::Error(error)] => {
                assert_eq!(error.major_code, X_RENDER_MAJOR_OPCODE);
                (error.code, error.minor_code)
            }
            other => panic!("minor {minor} produced {other:?}"),
        }
    };

    // Within the advertised 0.4: the never-implemented five, the declined
    // trapezoid family, and the base requests still to be implemented.
    // Within the advertised 0.10 and not implemented: the five no server
    // ever wrote, plus indexed-visual palettes, animated cursors and
    // AddTraps -- declined by name rather than by silence.
    for minor in [2, 3, 9, 14, 15, 21, 31, 32] {
        let (code, named) = refusal_for(minor);
        assert_eq!(code, XErrorCode::BadImplementation, "minor {minor}");
        assert_eq!(named, u16::from(minor));
    }
    // Beyond 0.4, or defined by no version at all.
    // Defined by no version of the protocol at all.
    for minor in [16, 37, 99] {
        let (code, named) = refusal_for(minor);
        assert_eq!(code, XErrorCode::BadRequest, "minor {minor}");
        assert_eq!(named, u16::from(minor));
    }
}

/// A fixture that drives RENDER requests against one runtime and reads the
/// resulting pixels back.
struct RenderFixture {
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    sequence: u16,
}

impl RenderFixture {
    const NS: NamespaceId = NamespaceId::from_raw(88);
    const ORDER: XByteOrder = XByteOrder::LittleEndian;
    const PIXMAP: u32 = 0x0020_0100;
    const PICTURE: u32 = 0x0020_0101;

    fn new() -> Self {
        Self {
            runtime: XAuthorityRuntime::new(),
            atoms: XAtomTable::new(),
            properties: XPropertyTable::new(),
            sequence: 0,
        }
    }

    fn send(&mut self, bytes: &[u8]) -> XDispatchResult {
        self.sequence = self.sequence.wrapping_add(1);
        let request = decode_x11_core_request(
            context(Self::NS, u64::from(self.sequence) + 900, Self::ORDER),
            bytes,
        )
        .expect("request must decode");
        dispatch_x11_wire_request(
            dispatch_context(Self::NS, self.sequence, Self::ORDER, bytes[0]),
            request,
            &mut self.runtime,
            &mut self.atoms,
            &mut self.properties,
        )
    }

    /// A depth-32 pixmap with an ARGB32 picture bound to it.
    fn with_argb_pixmap(width: u16, height: u16) -> Self {
        let mut fixture = Self::new();
        let create = create_pixmap_request(
            Self::ORDER,
            32,
            Self::PIXMAP,
            X_SETUP_DEFAULT_ROOT,
            width,
            height,
        );
        assert!(fixture.send(&create).outputs.is_empty(), "pixmap create");
        let picture = render_create_picture_request(
            Self::ORDER,
            Self::PICTURE,
            Self::PIXMAP,
            X_RENDER_FORMAT_ARGB32,
            &[],
        );
        assert!(fixture.send(&picture).outputs.is_empty(), "picture create");
        fixture
    }

    fn error_of(result: &XDispatchResult) -> Option<XErrorCode> {
        result.outputs.iter().find_map(|output| match output {
            XClientOutput::Error(error) => Some(error.code),
            _ => None,
        })
    }

    /// One pixel of the pixmap as `[b, g, r, a]`.
    fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        let bytes = self
            .runtime
            .drawable_image_region(
                Self::NS,
                XResourceId::new(u64::from(Self::PIXMAP), 1),
                Rect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                },
            )
            .expect("pixmap must have backing");
        [bytes[0], bytes[1], bytes[2], bytes[3]]
    }
}

/// FillRectangles blends premultiplied color the way the protocol defines,
/// and the bytes in the drawable are the ones a client can hand-compute.
///
/// The store had no alpha semantics before this: every core drawing operation
/// masks the top byte away. A picture over a depth-32 pixmap is where alpha
/// becomes real, and asserting exact bytes rather than "something changed" is
/// what makes the operator table falsifiable.
#[test]
fn render_fill_rectangles_blends_premultiplied_color_into_the_drawable() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    let whole = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };

    // Src writes the color through, ignoring what was there.
    let opaque_red = [0xffff, 0, 0, 0xffff];
    let result = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        opaque_red,
        &[whole],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(1, 1), [0, 0, 0xff, 0xff], "Src red");

    // Over with a half-alpha premultiplied blue: the protocol's result is
    // src + dst * (1 - src_alpha). With src = (0x80,0,0,0x80) over
    // (0,0,0xff,0xff): blue 0x80 + 0 = 0x80, red 0 + 0xff*0x7f/0xff = 0x7f,
    // alpha 0x80 + 0xff*0x7f/0xff = 0xff.
    let half_blue = [0, 0, 0x8080, 0x8080];
    let result = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::PICTURE,
        half_blue,
        &[whole],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(1, 1), [0x80, 0, 0x7f, 0xff], "Over blue");

    // Clear zeroes every channel, alpha included -- the one operator that
    // proves the alpha byte is genuinely being written rather than defaulted.
    let result = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        0,
        RenderFixture::PICTURE,
        opaque_red,
        &[whole],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(1, 1), [0, 0, 0, 0], "Clear");
}

/// A picture's clip list bounds what its fills touch.
#[test]
fn render_picture_clip_rectangles_bound_what_a_fill_touches() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    let result = fixture.send(&render_set_picture_clip_rectangles_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        1,
        1,
        &[Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        }],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    let result = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        }],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    // The clip origin shifts the rectangle to cover (1,1)..(3,3).
    assert_eq!(fixture.pixel(1, 1), [0xff, 0xff, 0xff, 0xff], "inside clip");
    assert_eq!(fixture.pixel(0, 0), [0, 0, 0, 0], "outside clip");
    assert_eq!(fixture.pixel(3, 3), [0, 0, 0, 0], "outside clip");
}

/// Pictures are refused, and die, on the terms the protocol sets.
#[test]
fn render_pictures_are_refused_and_reclaimed_on_protocol_terms() {
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);

    // A format whose depth is not the drawable's would read colour bytes as
    // coverage; BadMatch is the protocol's answer.
    let mismatched = render_create_picture_request(
        RenderFixture::ORDER,
        0x0020_0200,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_A8,
        &[],
    );
    let result = fixture.send(&mismatched);
    assert_eq!(RenderFixture::error_of(&result), Some(XErrorCode::BadMatch));

    // An unknown format id gets the extension's own error, not BadValue.
    let unknown = render_create_picture_request(
        RenderFixture::ORDER,
        0x0020_0201,
        RenderFixture::PIXMAP,
        0x1234,
        &[],
    );
    let result = fixture.send(&unknown);
    assert_eq!(
        RenderFixture::error_of(&result),
        Some(XErrorCode::RenderPictFormat)
    );

    // Reusing a live id is BadIdChoice, as for any resource.
    let duplicate = render_create_picture_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_ARGB32,
        &[],
    );
    let result = fixture.send(&duplicate);
    assert_eq!(
        RenderFixture::error_of(&result),
        Some(XErrorCode::BadIdChoice)
    );

    // Alpha maps are declined by name rather than silently dropped: dropping
    // one changes what the client drew without telling it.
    let alpha_map = render_create_picture_request(
        RenderFixture::ORDER,
        0x0020_0202,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_ARGB32,
        &[(1, RenderFixture::PIXMAP)],
    );
    let result = fixture.send(&alpha_map);
    assert_eq!(
        RenderFixture::error_of(&result),
        Some(XErrorCode::BadImplementation)
    );

    // The protocol defines four repeat modes; a fifth is not one of them.
    let undefined_repeat = render_create_picture_request(
        RenderFixture::ORDER,
        0x0020_0203,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_ARGB32,
        &[(0, 4)],
    );
    let result = fixture.send(&undefined_repeat);
    assert_eq!(RenderFixture::error_of(&result), Some(XErrorCode::BadValue));

    // An operator the protocol defines and this server withholds is refused
    // as unimplemented; one no version defines gets the PictOp error.
    let disjoint = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        0x13,
        RenderFixture::PICTURE,
        [0, 0, 0, 0],
        &[Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }],
    ));
    assert_eq!(
        RenderFixture::error_of(&disjoint),
        Some(XErrorCode::BadImplementation)
    );
    let undefined = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        0x2e,
        RenderFixture::PICTURE,
        [0, 0, 0, 0],
        &[Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }],
    ));
    assert_eq!(
        RenderFixture::error_of(&undefined),
        Some(XErrorCode::RenderPictOp)
    );

    // Freeing the picture releases the id; using it afterwards is refused
    // with the extension's Picture error.
    let free = fixture.send(&render_free_picture_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
    ));
    assert_eq!(RenderFixture::error_of(&free), None);
    let after_free = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        [0, 0, 0, 0],
        &[Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }],
    ));
    assert_eq!(
        RenderFixture::error_of(&after_free),
        Some(XErrorCode::RenderPicture)
    );
}

impl RenderFixture {
    const SOURCE_PIXMAP: u32 = 0x0020_0110;
    const SOURCE_PICTURE: u32 = 0x0020_0111;

    /// A second depth-32 pixmap and picture, to composite from.
    fn add_source(&mut self, width: u16, height: u16, repeat: bool) {
        let create = create_pixmap_request(
            Self::ORDER,
            32,
            Self::SOURCE_PIXMAP,
            X_SETUP_DEFAULT_ROOT,
            width,
            height,
        );
        assert!(self.send(&create).outputs.is_empty(), "source pixmap");
        let values: &[(u32, u32)] = if repeat { &[(0, 1)] } else { &[] };
        let picture = render_create_picture_request(
            Self::ORDER,
            Self::SOURCE_PICTURE,
            Self::SOURCE_PIXMAP,
            X_RENDER_FORMAT_ARGB32,
            values,
        );
        assert!(self.send(&picture).outputs.is_empty(), "source picture");
    }

    /// Fill the source picture with one premultiplied colour, using Src.
    fn fill_source(&mut self, color: [u16; 4], rect: Rect) {
        let request = render_fill_rectangles_request(Self::ORDER, 1, Self::SOURCE_PICTURE, color, &[rect]);
        assert!(self.send(&request).outputs.is_empty(), "source fill");
    }

    fn fill_destination(&mut self, color: [u16; 4], rect: Rect) {
        let request = render_fill_rectangles_request(Self::ORDER, 1, Self::PICTURE, color, &[rect]);
        assert!(self.send(&request).outputs.is_empty(), "destination fill");
    }
}

/// Composite blends a source picture over a destination, and the resulting
/// bytes are the ones the protocol's formula produces.
#[test]
fn render_composite_blends_a_source_picture_over_a_destination() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    let whole = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };
    fixture.add_source(4, 4, false);
    // Half-alpha premultiplied blue over opaque red, the same arithmetic the
    // fill test verifies, now carried through a sampled source plane.
    fixture.fill_source([0, 0, 0x8080, 0x8080], whole);
    fixture.fill_destination([0xffff, 0, 0, 0xffff], whole);

    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        0,
        0,
        4,
        4,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(2, 2), [0x80, 0, 0x7f, 0xff]);
}

/// A one-pixel repeating picture covers a whole destination.
///
/// This is how every toolkit paints a solid colour before CreateSolidFill
/// existed, and CreateSolidFill entered at 0.10 -- above what is advertised
/// -- so for a client talking to this server it is the only way.
#[test]
fn render_composite_repeats_a_one_pixel_source_across_the_destination() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    fixture.add_source(1, 1, true);
    fixture.fill_source(
        [0, 0xffff, 0, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );

    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::SOURCE_PICTURE,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        0,
        0,
        4,
        4,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    for (x, y) in [(0, 0), (3, 3), (1, 2)] {
        assert_eq!(fixture.pixel(x, y), [0, 0xff, 0, 0xff], "at {x},{y}");
    }

    // A non-repeating source of the same size reads transparent black
    // outside its one pixel, which is the protocol's other answer.
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    fixture.add_source(1, 1, false);
    fixture.fill_source(
        [0, 0xffff, 0, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::SOURCE_PICTURE,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        0,
        0,
        4,
        4,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(0, 0), [0, 0xff, 0, 0xff]);
    assert_eq!(fixture.pixel(3, 3), [0, 0, 0, 0], "outside a bounded source");
}

/// Compositing a picture onto itself reads the pixels it started with.
///
/// A client scrolling a window sends exactly this. Sampling into an owned
/// plane before writing is what makes the answer independent of the
/// direction the loop runs; reading the destination live would smear the
/// overlapping region.
#[test]
fn render_composite_onto_itself_reads_the_pixels_it_started_with() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
    // A distinct value in the leftmost column only.
    fixture.fill_destination(
        [0xffff, 0, 0, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    // Shift right by one: each destination pixel takes its left neighbour.
    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        1,
        0,
        3,
        1,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(1, 0), [0, 0, 0xff, 0xff], "shifted red");
    // If the destination had been read live, the red would have smeared
    // across every column instead of moving one place.
    assert_eq!(fixture.pixel(2, 0), [0, 0, 0, 0], "must not smear");
    assert_eq!(fixture.pixel(3, 0), [0, 0, 0, 0], "must not smear");
}

/// A mask attenuates the source, and a component-alpha mask attenuates each
/// channel separately.
///
/// The second is the subpixel-antialiasing path Xft uses when configured for
/// LCD filtering. Treating it as a plain mask renders text with colour
/// fringes that read as a display fault rather than a server one, which is
/// why it is implemented rather than ignored.
#[test]
fn render_composite_masks_attenuate_the_source_per_channel_when_asked() {
    let mask_pixmap = 0x0020_0120;
    let mask_picture = 0x0020_0121;
    let whole = Rect {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
    };

    for component_alpha in [false, true] {
        let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
        fixture.add_source(2, 2, false);
        fixture.fill_source([0xffff, 0xffff, 0xffff, 0xffff], whole);

        let create = create_pixmap_request(
            RenderFixture::ORDER,
            32,
            mask_pixmap,
            X_SETUP_DEFAULT_ROOT,
            2,
            2,
        );
        assert!(fixture.send(&create).outputs.is_empty());
        let values: &[(u32, u32)] = if component_alpha { &[(12, 1)] } else { &[] };
        let picture = render_create_picture_request(
            RenderFixture::ORDER,
            mask_picture,
            mask_pixmap,
            X_RENDER_FORMAT_ARGB32,
            values,
        );
        assert!(fixture.send(&picture).outputs.is_empty());
        // A mask that is fully opaque in blue only: alpha 0xff, blue 0xff,
        // green and red zero.
        let fill = render_fill_rectangles_request(
            RenderFixture::ORDER,
            1,
            mask_picture,
            [0, 0, 0xffff, 0xffff],
            &[whole],
        );
        assert!(fixture.send(&fill).outputs.is_empty());

        let result = fixture.send(&render_composite_request(
            RenderFixture::ORDER,
            3,
            RenderFixture::SOURCE_PICTURE,
            mask_picture,
            RenderFixture::PICTURE,
            0,
            0,
            0,
            0,
            0,
            0,
            2,
            2,
        ));
        assert_eq!(RenderFixture::error_of(&result), None);
        if component_alpha {
            // Only the blue channel is covered, so only blue survives.
            assert_eq!(fixture.pixel(0, 0), [0xff, 0, 0, 0xff]);
        } else {
            // The mask's alpha is opaque, so the white source passes whole.
            assert_eq!(fixture.pixel(0, 0), [0xff, 0xff, 0xff, 0xff]);
        }
    }
}

/// Compositing onto an RGB24 destination discards the result's alpha.
///
/// The format has no alpha component, so the protocol defines the result
/// that way; the store's slot keeps a zero alpha byte and the window buffer
/// tag stays XR24, which is what the compositor was promised.
#[test]
fn render_composite_onto_an_opaque_format_discards_result_alpha() {
    let mut fixture = RenderFixture::new();
    let create = create_pixmap_request(
        RenderFixture::ORDER,
        24,
        RenderFixture::PIXMAP,
        X_SETUP_DEFAULT_ROOT,
        2,
        2,
    );
    assert!(fixture.send(&create).outputs.is_empty());
    let picture = render_create_picture_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_RGB24,
        &[],
    );
    assert!(fixture.send(&picture).outputs.is_empty());

    let result = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        [0, 0, 0xffff, 0xffff],
        &[Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        }],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(0, 0), [0xff, 0, 0, 0], "alpha byte stays zero");
}

impl RenderFixture {
    const GLYPHSET: u32 = 0x0020_0130;

    fn add_glyphset(&mut self, format: u32) {
        let request = render_create_glyph_set_request(Self::ORDER, Self::GLYPHSET, format);
        assert!(self.send(&request).outputs.is_empty(), "glyph set create");
    }
}

/// Antialiased glyph coverage attenuates the source colour, which is what
/// makes text drawn through RENDER look like text rather than a bitmap.
///
/// The A8 coverage byte is the whole point of the extension for a toolkit:
/// a client uploads partial coverage at a glyph's edges and expects the
/// server to blend it, and asserting the blended byte is what proves the
/// coverage was honoured rather than thresholded.
#[test]
fn render_composite_glyphs_blends_coverage_into_the_destination() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    fixture.add_source(1, 1, true);
    // An opaque red source, repeating, which is how a client paints text in
    // one colour.
    fixture.fill_source(
        [0xffff, 0, 0, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    fixture.add_glyphset(X_RENDER_FORMAT_A8);

    // A 2x1 glyph: one pixel fully covered, one half covered. The A8 stride
    // pads to four bytes.
    let request = render_add_glyphs_request(
        RenderFixture::ORDER,
        RenderFixture::GLYPHSET,
        &[(7, [2, 1], [0, 0, 2, 0], vec![0xff, 0x80, 0, 0])],
    );
    assert!(fixture.send(&request).outputs.is_empty(), "add glyphs");

    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        RenderFixture::GLYPHSET,
        0,
        0,
        (1, 1),
        &[7],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);

    // Full coverage passes the source through unchanged.
    assert_eq!(fixture.pixel(1, 1), [0, 0, 0xff, 0xff], "covered pixel");
    // Half coverage scales the premultiplied source: 0xff * 0x80 / 255 = 0x80
    // in both the red channel and alpha, over a transparent destination.
    assert_eq!(fixture.pixel(2, 1), [0, 0, 0x80, 0x80], "half-covered pixel");
    // Outside the glyph nothing was drawn.
    assert_eq!(fixture.pixel(3, 1), [0, 0, 0, 0], "beyond the glyph");
    assert_eq!(fixture.pixel(1, 0), [0, 0, 0, 0], "above the glyph");
}

/// A referenced glyph set shares storage with the set it names.
///
/// The protocol says the second name refers to the same glyphs rather than
/// copying them, so a glyph added through one name is visible through the
/// other, and the contents survive until the last name is freed. A client
/// that frees the original and keeps drawing through the reference is doing
/// something the protocol allows.
#[test]
fn render_referenced_glyph_sets_share_storage_and_outlive_the_first_name() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    fixture.add_source(1, 1, true);
    fixture.fill_source(
        [0xffff, 0xffff, 0xffff, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    fixture.add_glyphset(X_RENDER_FORMAT_A8);

    let reference = 0x0020_0131;
    let request =
        render_reference_glyph_set_request(RenderFixture::ORDER, reference, RenderFixture::GLYPHSET);
    assert!(fixture.send(&request).outputs.is_empty(), "reference");

    // Added through the original name.
    let add = render_add_glyphs_request(
        RenderFixture::ORDER,
        RenderFixture::GLYPHSET,
        &[(3, [1, 1], [0, 0, 1, 0], vec![0xff, 0, 0, 0])],
    );
    assert!(fixture.send(&add).outputs.is_empty());

    // Freeing the original must not take the glyphs with it.
    let free = render_free_glyph_set_request(RenderFixture::ORDER, RenderFixture::GLYPHSET);
    assert!(fixture.send(&free).outputs.is_empty(), "free original");

    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        reference,
        0,
        0,
        (0, 0),
        &[3],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(0, 0), [0xff, 0xff, 0xff, 0xff]);

    // The original name is genuinely gone.
    let stale = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        RenderFixture::GLYPHSET,
        0,
        0,
        (0, 0),
        &[3],
    ));
    assert_eq!(
        RenderFixture::error_of(&stale),
        Some(XErrorCode::RenderGlyphSet)
    );
}

/// A run naming a glyph the set does not hold draws nothing at all.
///
/// Resolving every glyph before drawing any is what makes the refusal clean:
/// a client that gets an error and redraws would otherwise find the prefix
/// of its run already on screen and draw it twice.
#[test]
fn render_composite_glyphs_refuses_an_unknown_glyph_without_drawing_the_prefix() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    fixture.add_source(1, 1, true);
    fixture.fill_source(
        [0xffff, 0xffff, 0xffff, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    fixture.add_glyphset(X_RENDER_FORMAT_A8);
    let add = render_add_glyphs_request(
        RenderFixture::ORDER,
        RenderFixture::GLYPHSET,
        &[(1, [1, 1], [0, 0, 1, 0], vec![0xff, 0, 0, 0])],
    );
    assert!(fixture.send(&add).outputs.is_empty());

    // Glyph 1 exists, glyph 2 does not.
    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        RenderFixture::GLYPHSET,
        0,
        0,
        (0, 0),
        &[1, 2],
    ));
    assert_eq!(
        RenderFixture::error_of(&result),
        Some(XErrorCode::RenderGlyph)
    );
    assert_eq!(fixture.pixel(0, 0), [0, 0, 0, 0], "prefix must not draw");
}

/// AddGlyphs whose image bytes do not cover its glyph table is refused, and
/// leaves the set untouched.
#[test]
fn render_add_glyphs_refuses_data_shorter_than_its_glyph_table() {
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    fixture.add_glyphset(X_RENDER_FORMAT_A8);
    // A 4x4 A8 glyph needs sixteen bytes; four are supplied.
    let request = render_add_glyphs_request(
        RenderFixture::ORDER,
        RenderFixture::GLYPHSET,
        &[(9, [4, 4], [0, 0, 4, 0], vec![0xff, 0xff, 0xff, 0xff])],
    );
    assert_eq!(
        RenderFixture::error_of(&fixture.send(&request)),
        Some(XErrorCode::BadLength)
    );
}

/// RENDER is advertised, with its own error base, now that the requests
/// behind the advertised version answer.
#[test]
fn render_is_advertised_once_its_requests_answer() {
    let mut fixture = RenderFixture::new();
    let result = fixture.send(&query_extension_request(
        RenderFixture::ORDER,
        X_RENDER_EXTENSION_NAME,
    ));
    let encoded = result.encoded_outputs(RenderFixture::ORDER);
    assert_eq!(encoded[0][8], 1, "present");
    assert_eq!(encoded[0][9], X_RENDER_MAJOR_OPCODE);
    assert_eq!(encoded[0][11], X_RENDER_FIRST_ERROR, "first error");
}

/// A RENDER cursor stores the picture's premultiplied pixels, and FreeCursor
/// releases them.
///
/// RENDER pictures are premultiplied already, which is the engine's
/// `CursorAsset` contract exactly -- unlike core `CreateCursor`, whose source
/// and mask bitmaps carry no alpha at all. Storing the image makes the
/// resource real; putting it on screen is a separate step, and the cursor the
/// compositor draws stays the configured one until that lands.
#[test]
fn render_cursors_store_the_pictures_premultiplied_pixels() {
    let cursor = 0x0020_0140;
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    // Half-alpha red, premultiplied, filling the picture.
    fixture.fill_destination(
        [0x8080, 0, 0, 0x8080],
        Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        },
    );
    let result = fixture.send(&render_create_cursor_request(
        RenderFixture::ORDER,
        cursor,
        RenderFixture::PICTURE,
        1,
        1,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    let image = fixture
        .runtime
        .render_cursor_image(XResourceId::new(u64::from(cursor), 1))
        .expect("the cursor image must be stored");
    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!((image.hotspot_x, image.hotspot_y), (1, 1));
    // Stored exactly as the picture holds them: premultiplied [b, g, r, a].
    assert_eq!(&image.premultiplied_bgra[0..4], &[0, 0, 0x80, 0x80]);
    // Every channel is at or below its alpha, which is the invariant the
    // engine validates -- so a stored image is always one it could accept.
    for pixel in image.premultiplied_bgra.chunks_exact(4) {
        assert!(
            pixel[0..3].iter().all(|channel| *channel <= pixel[3]),
            "not premultiplied: {pixel:?}"
        );
    }

    let free = fixture.send(&free_cursor_request(RenderFixture::ORDER, cursor));
    assert_eq!(RenderFixture::error_of(&free), None);
    assert!(
        fixture
            .runtime
            .render_cursor_image(XResourceId::new(u64::from(cursor), 1))
            .is_none(),
        "FreeCursor must release the image"
    );
}

/// A cursor is refused on the terms that keep the stored image usable.
#[test]
fn render_cursors_are_refused_when_the_picture_cannot_describe_one() {
    // A picture with no alpha cannot describe a cursor's shape.
    let mut fixture = RenderFixture::new();
    let create = create_pixmap_request(
        RenderFixture::ORDER,
        24,
        RenderFixture::PIXMAP,
        X_SETUP_DEFAULT_ROOT,
        2,
        2,
    );
    assert!(fixture.send(&create).outputs.is_empty());
    let picture = render_create_picture_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_RGB24,
        &[],
    );
    assert!(fixture.send(&picture).outputs.is_empty());
    let result = fixture.send(&render_create_cursor_request(
        RenderFixture::ORDER,
        0x0020_0141,
        RenderFixture::PICTURE,
        0,
        0,
    ));
    assert_eq!(RenderFixture::error_of(&result), Some(XErrorCode::BadMatch));

    // A hotspot outside the image would point somewhere the cursor is not.
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    let result = fixture.send(&render_create_cursor_request(
        RenderFixture::ORDER,
        0x0020_0142,
        RenderFixture::PICTURE,
        2,
        0,
    ));
    assert_eq!(RenderFixture::error_of(&result), Some(XErrorCode::BadValue));

    // Larger than the engine accepts is refused rather than scaled: a cursor
    // silently resized is one whose hotspot no longer points where the client
    // put it.
    let mut fixture = RenderFixture::with_argb_pixmap(200, 200);
    let result = fixture.send(&render_create_cursor_request(
        RenderFixture::ORDER,
        0x0020_0143,
        RenderFixture::PICTURE,
        0,
        0,
    ));
    assert_eq!(RenderFixture::error_of(&result), Some(XErrorCode::BadAlloc));
}

/// A fixture driving XFIXES region requests against one runtime.
struct XfixesRegionFixture {
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    sequence: u16,
}

impl XfixesRegionFixture {
    const NS: NamespaceId = NamespaceId::from_raw(91);
    const ORDER: XByteOrder = XByteOrder::LittleEndian;
    const A: u32 = 0x0020_0300;
    const B: u32 = 0x0020_0301;
    const OUT: u32 = 0x0020_0302;

    fn new() -> Self {
        Self {
            runtime: XAuthorityRuntime::new(),
            atoms: XAtomTable::new(),
            properties: XPropertyTable::new(),
            sequence: 0,
        }
    }

    fn send(&mut self, bytes: &[u8]) -> XDispatchResult {
        self.sequence = self.sequence.wrapping_add(1);
        let request = decode_x11_core_request(
            context(Self::NS, u64::from(self.sequence) + 1200, Self::ORDER),
            bytes,
        )
        .expect("request must decode");
        dispatch_x11_wire_request(
            dispatch_context(Self::NS, self.sequence, Self::ORDER, bytes[0]),
            request,
            &mut self.runtime,
            &mut self.atoms,
            &mut self.properties,
        )
    }

    fn create(&mut self, id: u32, rects: &[Rect]) {
        let request = xfixes_create_region_request(Self::ORDER, id, rects);
        assert!(self.send(&request).outputs.is_empty(), "create {id:#x}");
    }

    /// The region's rectangles, read back the way a client reads them.
    fn fetch(&mut self, id: u32) -> Vec<Rect> {
        let result = self.send(&xfixes_fetch_region_request(Self::ORDER, id));
        match result.outputs.as_slice() {
            [XClientOutput::Reply(XClientReply::XfixesFetchRegion { rects, .. })] => rects.clone(),
            other => panic!("fetch produced {other:?}"),
        }
    }

    fn error_of(result: &XDispatchResult) -> Option<XErrorCode> {
        result.outputs.iter().find_map(|output| match output {
            XClientOutput::Error(error) => Some(error.code),
            _ => None,
        })
    }
}

/// The region operations combine what the client asked them to, and the
/// result reads back canonically.
///
/// XFIXES has answered version 6.0 since before these existed, so a client
/// that believed the version and tried to compute with a region got a parse
/// failure. These are the minors that make a region a value rather than a
/// container.
#[test]
fn xfixes_regions_combine_and_read_back_canonically() {
    let mut fixture = XfixesRegionFixture::new();
    let left = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };
    let right = Rect {
        x: 2,
        y: 0,
        width: 4,
        height: 4,
    };
    fixture.create(XfixesRegionFixture::A, &[left]);
    fixture.create(XfixesRegionFixture::B, &[right]);
    fixture.create(XfixesRegionFixture::OUT, &[]);

    let union = fixture.send(&xfixes_combine_region_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_UNION_REGION_MINOR_OPCODE,
        XfixesRegionFixture::A,
        XfixesRegionFixture::B,
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&union), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 0,
            y: 0,
            width: 6,
            height: 4,
        }],
        "the union is one rect, not two overlapping ones"
    );

    let intersect = fixture.send(&xfixes_combine_region_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_INTERSECT_REGION_MINOR_OPCODE,
        XfixesRegionFixture::A,
        XfixesRegionFixture::B,
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&intersect), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 2,
            y: 0,
            width: 2,
            height: 4,
        }]
    );

    let subtract = fixture.send(&xfixes_combine_region_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_SUBTRACT_REGION_MINOR_OPCODE,
        XfixesRegionFixture::A,
        XfixesRegionFixture::B,
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&subtract), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 4,
        }]
    );

    // Copy carries one source across and canonicalises on the way.
    let copy = fixture.send(&xfixes_combine_region_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_COPY_REGION_MINOR_OPCODE,
        XfixesRegionFixture::A,
        0,
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&copy), None);
    assert_eq!(fixture.fetch(XfixesRegionFixture::OUT), vec![left]);
}

/// A destination that names one of its own sources still means what the
/// client asked.
///
/// `UnionRegion(a, b, a)` is ordinary client code, and an implementation
/// that wrote the destination while still reading it would answer from
/// half-updated state.
#[test]
fn xfixes_region_operations_allow_the_destination_to_be_a_source() {
    let mut fixture = XfixesRegionFixture::new();
    fixture.create(
        XfixesRegionFixture::A,
        &[Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        }],
    );
    fixture.create(
        XfixesRegionFixture::B,
        &[Rect {
            x: 4,
            y: 0,
            width: 4,
            height: 4,
        }],
    );
    let result = fixture.send(&xfixes_combine_region_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_UNION_REGION_MINOR_OPCODE,
        XfixesRegionFixture::A,
        XfixesRegionFixture::B,
        XfixesRegionFixture::A,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&result), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::A),
        vec![Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 4,
        }]
    );
}

/// Invert, translate and extents answer what the protocol defines.
#[test]
fn xfixes_invert_translate_and_extents_answer_the_protocol() {
    let mut fixture = XfixesRegionFixture::new();
    // A hole in the middle of a square: invert is the source subtracted from
    // the bounds the client supplies, because a region has no complement
    // without them.
    fixture.create(
        XfixesRegionFixture::A,
        &[Rect {
            x: 2,
            y: 2,
            width: 2,
            height: 2,
        }],
    );
    fixture.create(XfixesRegionFixture::OUT, &[]);
    let invert = fixture.send(&xfixes_invert_region_request(
        XfixesRegionFixture::ORDER,
        XfixesRegionFixture::A,
        Rect {
            x: 0,
            y: 0,
            width: 6,
            height: 6,
        },
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&invert), None);
    let frame = fixture.fetch(XfixesRegionFixture::OUT);
    let area: i32 = frame.iter().map(|r| r.width * r.height).sum();
    assert_eq!(area, 32, "a frame, not the whole square and not nothing");

    let translate = fixture.send(&xfixes_translate_region_request(
        XfixesRegionFixture::ORDER,
        XfixesRegionFixture::A,
        10,
        20,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&translate), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::A),
        vec![Rect {
            x: 12,
            y: 22,
            width: 2,
            height: 2,
        }]
    );

    fixture.create(
        XfixesRegionFixture::B,
        &[
            Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
            Rect {
                x: 8,
                y: 6,
                width: 2,
                height: 2,
            },
        ],
    );
    let extents = fixture.send(&xfixes_region_extents_request(
        XfixesRegionFixture::ORDER,
        XfixesRegionFixture::B,
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&extents), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 8,
        }]
    );
}

/// An XFIXES minor with no implementation is refused by name rather than
/// failing to parse.
///
/// This server answers XFIXES 6.0 and does not implement every minor behind
/// it. A parse rejection told a client only that the extension existed; a
/// named refusal says which request was declined, which is the discipline
/// every other extension here follows.
#[test]
fn xfixes_minors_without_an_implementation_are_refused_by_name() {
    let mut fixture = XfixesRegionFixture::new();
    // Defined by version 6.0, not implemented here. Minors 6 to 9 have left
    // this list; 20 to 22 install a region rather than build one and still
    // await the clip and shape plumbing.
    for minor in [20, 21, 22, 29, 32, 34] {
        let result = fixture.send(&xfixes_minor_request(XfixesRegionFixture::ORDER, minor));
        match result.outputs.as_slice() {
            [XClientOutput::Error(error)] => {
                assert_eq!(error.code, XErrorCode::BadImplementation, "minor {minor}");
                assert_eq!(error.minor_code, u16::from(minor));
                assert_eq!(error.major_code, X_XFIXES_MAJOR_OPCODE);
            }
            other => panic!("minor {minor} produced {other:?}"),
        }
    }
    // Beyond anything the version defines.
    for minor in [35, 200] {
        let result = fixture.send(&xfixes_minor_request(XfixesRegionFixture::ORDER, minor));
        assert_eq!(
            XfixesRegionFixture::error_of(&result),
            Some(XErrorCode::BadRequest),
            "minor {minor}"
        );
    }
}

/// A fixture driving SHAPE requests against one window.
struct ShapeFixture {
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    sequence: u16,
}

impl ShapeFixture {
    const NS: NamespaceId = NamespaceId::from_raw(93);
    const ORDER: XByteOrder = XByteOrder::LittleEndian;
    const WINDOW: u32 = 0x0020_0400;
    const OTHER: u32 = 0x0020_0401;
    const MASK: u32 = 0x0020_0402;
    const CLIENT: u64 = 77;

    /// A ten-by-ten window, which every default region below is measured
    /// against.
    fn new() -> Self {
        let mut fixture = Self {
            runtime: XAuthorityRuntime::new(),
            atoms: XAtomTable::new(),
            properties: XPropertyTable::new(),
            sequence: 0,
        };
        let create = create_window_request(Self::ORDER, Self::WINDOW, 0, 0, 10, 10);
        let result = fixture.send(&create);
        assert_eq!(Self::error_of(&result), None, "window create");
        fixture
    }

    fn send(&mut self, bytes: &[u8]) -> XDispatchResult {
        self.send_as(Self::CLIENT, bytes)
    }

    fn send_as(&mut self, client: u64, bytes: &[u8]) -> XDispatchResult {
        self.sequence = self.sequence.wrapping_add(1);
        let request = decode_x11_core_request(
            context(Self::NS, u64::from(self.sequence) + 1400, Self::ORDER),
            bytes,
        )
        .expect("request must decode");
        let mut dispatch = dispatch_context(Self::NS, self.sequence, Self::ORDER, bytes[0]);
        dispatch.client_id = client;
        dispatch_x11_wire_request(
            dispatch,
            request,
            &mut self.runtime,
            &mut self.atoms,
            &mut self.properties,
        )
    }

    /// Set one kind to a rectangle list.
    fn set(&mut self, kind: u8, rects: &[Rect]) -> XDispatchResult {
        self.send(&shape_rectangles_request(
            Self::ORDER,
            X_SHAPE_OP_SET,
            kind,
            X_SHAPE_ORDERING_UNSORTED,
            Self::WINDOW,
            0,
            0,
            rects,
        ))
    }

    fn rects(&mut self, kind: u8) -> Vec<Rect> {
        let result = self.send(&shape_get_rectangles_request(Self::ORDER, Self::WINDOW, kind));
        match result.outputs.as_slice() {
            [XClientOutput::Reply(XClientReply::ShapeGetRectangles { rects, ordering, .. })] => {
                assert_eq!(*ordering, X_SHAPE_ORDERING_YX_BANDED);
                rects.clone()
            }
            other => panic!("GetRectangles produced {other:?}"),
        }
    }

    /// Whether each kind reports itself shaped, and its extents.
    fn extents(&mut self) -> (bool, bool, Rect) {
        let result = self.send(&shape_query_extents_request(Self::ORDER, Self::WINDOW));
        match result.outputs.as_slice() {
            [XClientOutput::Reply(XClientReply::ShapeQueryExtents {
                bounding_shaped,
                clip_shaped,
                bounding_extents,
                ..
            })] => (*bounding_shaped, *clip_shaped, *bounding_extents),
            other => panic!("QueryExtents produced {other:?}"),
        }
    }

    fn notify_of(result: &XDispatchResult) -> Option<(u8, bool, Rect)> {
        result.outputs.iter().find_map(|output| match output {
            XClientOutput::Event(XClientEvent::ShapeNotify {
                kind,
                shaped,
                extents,
                ..
            }) => Some((*kind, *shaped, *extents)),
            _ => None,
        })
    }

    fn error_of(result: &XDispatchResult) -> Option<XErrorCode> {
        result.outputs.iter().find_map(|output| match output {
            XClientOutput::Error(error) => Some(error.code),
            _ => None,
        })
    }
}

/// Unset, explicitly empty, and concrete are three different answers.
///
/// An unset kind reports the window's live bounds, so a resize moves the
/// answer without anything writing the store -- materialising the default
/// instead freezes an extent that stops tracking geometry, which is the bug
/// this tri-state exists to avoid. An explicitly empty shape is a client
/// asking for nothing, and must not read back as unset.
#[test]
fn shape_distinguishes_unset_from_empty_from_concrete() {
    let mut fixture = ShapeFixture::new();

    // Unset: the window's own bounds, and not reported as shaped.
    let (bounding_shaped, clip_shaped, extents) = fixture.extents();
    assert!(!bounding_shaped, "an untouched window is not shaped");
    assert!(!clip_shaped);
    assert_eq!(
        extents,
        Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10
        }
    );
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_BOUNDING),
        vec![Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10
        }],
        "an unset kind answers with the window rectangle"
    );

    // Explicitly empty: shaped, with nothing in it.
    let result = fixture.set(X_SHAPE_KIND_BOUNDING, &[]);
    assert_eq!(ShapeFixture::error_of(&result), None);
    let (shaped, _, extents) = fixture.extents();
    assert!(shaped, "an empty shape is still a shape");
    assert_eq!(extents, Rect::default(), "and its extents are nothing");
    assert!(fixture.rects(X_SHAPE_KIND_BOUNDING).is_empty());

    // Concrete.
    let half = Rect {
        x: 0,
        y: 0,
        width: 10,
        height: 5,
    };
    fixture.set(X_SHAPE_KIND_BOUNDING, &[half]);
    let (shaped, _, extents) = fixture.extents();
    assert!(shaped);
    assert_eq!(extents, half);
    assert_eq!(fixture.rects(X_SHAPE_KIND_BOUNDING), vec![half]);

    // The three kinds are independent: shaping bounding left input alone.
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_INPUT),
        vec![Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10
        }]
    );
}

/// Each operation combines the way the protocol defines, including the two
/// that differ only in which side is subtracted.
///
/// Invert is the source with the destination taken out of it -- the mirror of
/// Subtract. At least one other implementation aliases it to Set, which is
/// silently wrong for any client that uses it.
#[test]
fn shape_operations_combine_as_the_protocol_defines() {
    let left = Rect {
        x: 0,
        y: 0,
        width: 6,
        height: 10,
    };
    let right = Rect {
        x: 4,
        y: 0,
        width: 6,
        height: 10,
    };

    let cases: &[(u8, &[Rect])] = &[
        (X_SHAPE_OP_SET, &[right]),
        (
            X_SHAPE_OP_UNION,
            &[Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            }],
        ),
        (
            X_SHAPE_OP_INTERSECT,
            &[Rect {
                x: 4,
                y: 0,
                width: 2,
                height: 10,
            }],
        ),
        // Subtract: the destination without the source.
        (
            X_SHAPE_OP_SUBTRACT,
            &[Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 10,
            }],
        ),
        // Invert: the source without the destination.
        (
            X_SHAPE_OP_INVERT,
            &[Rect {
                x: 6,
                y: 0,
                width: 4,
                height: 10,
            }],
        ),
    ];

    for (op, expected) in cases {
        let mut fixture = ShapeFixture::new();
        fixture.set(X_SHAPE_KIND_BOUNDING, &[left]);
        let result = fixture.send(&shape_rectangles_request(
            ShapeFixture::ORDER,
            *op,
            X_SHAPE_KIND_BOUNDING,
            X_SHAPE_ORDERING_UNSORTED,
            ShapeFixture::WINDOW,
            0,
            0,
            &[right],
        ));
        assert_eq!(ShapeFixture::error_of(&result), None, "op {op}");
        assert_eq!(fixture.rects(X_SHAPE_KIND_BOUNDING), *expected, "op {op}");
    }
}

/// An operation against a kind that was never set becomes Set.
///
/// There is nothing to combine with, and combining against the default the
/// client never asked for would answer a question it did not pose.
#[test]
fn shape_operations_on_an_unset_kind_become_set() {
    let quarter = Rect {
        x: 0,
        y: 0,
        width: 5,
        height: 5,
    };
    for op in [
        X_SHAPE_OP_UNION,
        X_SHAPE_OP_INTERSECT,
        X_SHAPE_OP_SUBTRACT,
        X_SHAPE_OP_INVERT,
    ] {
        let mut fixture = ShapeFixture::new();
        let result = fixture.send(&shape_rectangles_request(
            ShapeFixture::ORDER,
            op,
            X_SHAPE_KIND_BOUNDING,
            X_SHAPE_ORDERING_UNSORTED,
            ShapeFixture::WINDOW,
            0,
            0,
            &[quarter],
        ));
        assert_eq!(ShapeFixture::error_of(&result), None, "op {op}");
        assert_eq!(fixture.rects(X_SHAPE_KIND_BOUNDING), vec![quarter], "op {op}");
    }
}

/// A shape that does not move produces no event.
///
/// Window managers re-assert the same shape constantly; a notify for each
/// re-assertion is what broke panel buttons in the implementation this
/// gating was taken from. Re-asserting the same area written differently
/// must also stay silent, which is what canonical region equality buys.
#[test]
fn shape_notify_fires_only_when_the_shape_actually_moves() {
    let mut fixture = ShapeFixture::new();
    let whole = Rect {
        x: 0,
        y: 0,
        width: 8,
        height: 8,
    };

    let first = fixture.set(X_SHAPE_KIND_BOUNDING, &[whole]);
    let (kind, shaped, extents) =
        ShapeFixture::notify_of(&first).expect("the first shape is a change");
    assert_eq!(kind, X_SHAPE_KIND_BOUNDING);
    assert!(shaped);
    assert_eq!(extents, whole);

    // The same shape again: nothing.
    let repeat = fixture.set(X_SHAPE_KIND_BOUNDING, &[whole]);
    assert!(ShapeFixture::notify_of(&repeat).is_none(), "re-assertion");

    // The same area, written as four pieces: still nothing, because the
    // region is compared as a region and not as a list.
    let quarters = [
        Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        Rect {
            x: 4,
            y: 0,
            width: 4,
            height: 4,
        },
        Rect {
            x: 0,
            y: 4,
            width: 4,
            height: 4,
        },
        Rect {
            x: 4,
            y: 4,
            width: 4,
            height: 4,
        },
    ];
    let respelled = fixture.set(X_SHAPE_KIND_BOUNDING, &quarters);
    assert!(
        ShapeFixture::notify_of(&respelled).is_none(),
        "the same area spelled differently is not a change"
    );

    // A real change fires.
    let moved = fixture.set(
        X_SHAPE_KIND_BOUNDING,
        &[Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 4,
        }],
    );
    assert!(ShapeFixture::notify_of(&moved).is_some());

    // Returning to unset covers the same area as the default here would not
    // -- but the kind going from set to unset is itself a change, because
    // QueryExtents reports it differently.
    let mut fixture = ShapeFixture::new();
    let full = Rect {
        x: 0,
        y: 0,
        width: 10,
        height: 10,
    };
    let set_to_default = fixture.set(X_SHAPE_KIND_BOUNDING, &[full]);
    assert!(
        ShapeFixture::notify_of(&set_to_default).is_some(),
        "setting a kind is a change even at the default area"
    );
    let cleared = fixture.send(&shape_mask_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        0,
        0,
        0,
    ));
    let (_, shaped, _) = ShapeFixture::notify_of(&cleared).expect("unsetting is a change");
    assert!(!shaped, "and it reports the kind as no longer shaped");
    assert!(!fixture.extents().0);
}

/// A mask's set bits become the shape.
#[test]
fn shape_mask_reads_a_depth_one_pixmap() {
    let mut fixture = ShapeFixture::new();
    let create = create_pixmap_request(
        ShapeFixture::ORDER,
        1,
        ShapeFixture::MASK,
        X_SETUP_DEFAULT_ROOT,
        4,
        2,
    );
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create)), None);

    // Two rows: the left half set on the first, nothing on the second.
    let gc = 0x0020_0410;
    let create_gc = create_gc_request(ShapeFixture::ORDER, gc, ShapeFixture::MASK);
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create_gc)), None);
    // A depth-1 ZPixmap packs one bit per pixel, least significant first,
    // with rows padded to four bytes: the first row has its two leftmost
    // pixels set, the second has none.
    let data: Vec<u8> = vec![0b0000_0011, 0, 0, 0, 0, 0, 0, 0];
    let put = put_image_request_at_depth(
        ShapeFixture::ORDER,
        1,
        ShapeFixture::MASK,
        gc,
        4,
        2,
        &data,
    );
    assert_eq!(ShapeFixture::error_of(&fixture.send(&put)), None, "mask upload");

    let result = fixture.send(&shape_mask_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        0,
        0,
        ShapeFixture::MASK,
    ));
    assert_eq!(ShapeFixture::error_of(&result), None);
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_BOUNDING),
        vec![Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 1
        }],
        "only the set bits, as one banded rectangle"
    );

    // A mask that is not one bit deep cannot describe a shape.
    let deep = 0x0020_0411;
    let create_deep = create_pixmap_request(
        ShapeFixture::ORDER,
        24,
        deep,
        X_SETUP_DEFAULT_ROOT,
        4,
        4,
    );
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create_deep)), None);
    let refused = fixture.send(&shape_mask_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        0,
        0,
        deep,
    ));
    assert_eq!(
        ShapeFixture::error_of(&refused),
        Some(XErrorCode::BadMatch),
        "a depth-24 mask"
    );
}

/// Combine sources another window's shape; Offset moves one; both validate.
#[test]
fn shape_combine_and_offset_use_another_windows_shape() {
    let mut fixture = ShapeFixture::new();
    let create = create_window_request(ShapeFixture::ORDER, ShapeFixture::OTHER, 0, 0, 4, 4);
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create)), None);

    // The other window is unset, so its effective shape is its own bounds.
    let combined = fixture.send(&shape_combine_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        0,
        0,
        ShapeFixture::OTHER,
    ));
    assert_eq!(ShapeFixture::error_of(&combined), None);
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_BOUNDING),
        vec![Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4
        }]
    );

    let offset = fixture.send(&shape_offset_request(
        ShapeFixture::ORDER,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        3,
        2,
    ));
    assert_eq!(ShapeFixture::error_of(&offset), None);
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_BOUNDING),
        vec![Rect {
            x: 3,
            y: 2,
            width: 4,
            height: 4
        }]
    );

    // Offsetting a kind that was never set leaves it unset rather than
    // materialising the default somewhere the client never put it.
    let untouched = fixture.send(&shape_offset_request(
        ShapeFixture::ORDER,
        X_SHAPE_KIND_INPUT,
        ShapeFixture::WINDOW,
        5,
        5,
    ));
    assert_eq!(ShapeFixture::error_of(&untouched), None);
    assert!(ShapeFixture::notify_of(&untouched).is_none());
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_INPUT),
        vec![Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10
        }],
        "still the window's own bounds"
    );
}

/// Selection is per client and per window, and answers itself.
#[test]
fn shape_input_selection_is_per_client_and_window() {
    let mut fixture = ShapeFixture::new();
    let selected = |fixture: &mut ShapeFixture, client: u64| {
        let request = shape_input_selected_request(ShapeFixture::ORDER, ShapeFixture::WINDOW);
        match fixture.send_as(client, &request).outputs.as_slice() {
            [XClientOutput::Reply(XClientReply::ShapeInputSelected { enabled, .. })] => *enabled,
            other => panic!("InputSelected produced {other:?}"),
        }
    };

    assert!(!selected(&mut fixture, ShapeFixture::CLIENT));
    let select = shape_select_input_request(ShapeFixture::ORDER, ShapeFixture::WINDOW, true);
    assert!(fixture.send(&select).outputs.is_empty());
    assert!(selected(&mut fixture, ShapeFixture::CLIENT));
    // Another client's interest is its own.
    assert!(!selected(&mut fixture, ShapeFixture::CLIENT + 1));

    let deselect = shape_select_input_request(ShapeFixture::ORDER, ShapeFixture::WINDOW, false);
    assert!(fixture.send(&deselect).outputs.is_empty());
    assert!(!selected(&mut fixture, ShapeFixture::CLIENT));
}

/// Bad arguments are refused with the code the protocol expects.
#[test]
fn shape_refuses_invalid_windows_kinds_and_operations() {
    let mut fixture = ShapeFixture::new();
    let unknown = 0x0020_04ff;

    let bad_window = fixture.send(&shape_rectangles_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        X_SHAPE_ORDERING_UNSORTED,
        unknown,
        0,
        0,
        &[],
    ));
    assert_eq!(
        ShapeFixture::error_of(&bad_window),
        Some(XErrorCode::BadWindow)
    );

    for (kind, op, ordering) in [(9, X_SHAPE_OP_SET, 0), (X_SHAPE_KIND_BOUNDING, 9, 0), (X_SHAPE_KIND_BOUNDING, X_SHAPE_OP_SET, 9)] {
        let result = fixture.send(&shape_rectangles_request(
            ShapeFixture::ORDER,
            op,
            kind,
            ordering,
            ShapeFixture::WINDOW,
            0,
            0,
            &[],
        ));
        assert_eq!(
            ShapeFixture::error_of(&result),
            Some(XErrorCode::BadValue),
            "kind {kind} op {op} ordering {ordering}"
        );
    }

    // No version of SHAPE has a minor above eight.
    for minor in [9, 200] {
        let result = fixture.send(&shape_minor_request(ShapeFixture::ORDER, minor));
        assert_eq!(
            ShapeFixture::error_of(&result),
            Some(XErrorCode::BadRequest),
            "minor {minor}"
        );
    }
}

/// SHAPE answers its requests and is advertised, with its event base.
///
/// The advertisement waited for input shapes to genuinely make clicks fall
/// through, because that is what the client asking for this extension does
/// with it.
#[test]
fn shape_is_advertised_once_its_shapes_take_effect() {
    let mut fixture = ShapeFixture::new();
    let version = fixture.send(&shape_minor_request(
        ShapeFixture::ORDER,
        X_SHAPE_QUERY_VERSION_MINOR_OPCODE,
    ));
    assert!(
        matches!(
            version.outputs.as_slice(),
            [XClientOutput::Reply(XClientReply::ShapeQueryVersion {
                major_version: 1,
                minor_version: 1,
                ..
            })]
        ),
        "{:?}",
        version.outputs
    );

    let query = fixture.send(&query_extension_request(
        ShapeFixture::ORDER,
        X_SHAPE_EXTENSION_NAME,
    ));
    let encoded = query.encoded_outputs(ShapeFixture::ORDER);
    assert_eq!(encoded[0][8], 1, "present");
    assert_eq!(encoded[0][9], X_SHAPE_MAJOR_OPCODE);
    assert_eq!(encoded[0][10], X_SHAPE_FIRST_EVENT, "event base");
}

/// A bounding shape clips what the window presents, through the alpha the
/// renderer already blends.
///
/// The presentation buffer is published as ARGB with the shaped-out pixels
/// cleared to fully transparent, which is what makes the hole show the
/// desktop rather than a black patch. Asserting the exact bytes is what
/// distinguishes a shape that clips from a shape that is merely stored.
#[test]
fn a_bounding_shape_clears_what_the_window_presents_outside_it() {
    let mut fixture = ShapeFixture::new();
    let gc = 0x0020_0420;
    let create_gc = create_gc_request(ShapeFixture::ORDER, gc, ShapeFixture::WINDOW);
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create_gc)), None);

    // Fill the window so there is something to clip.
    let fill = poly_fill_rectangle_request(
        ShapeFixture::ORDER,
        ShapeFixture::WINDOW,
        gc,
        &[(0, 0, 10, 10)],
    );
    assert_eq!(ShapeFixture::error_of(&fixture.send(&fill)), None);

    // Shape it to its left half.
    let result = fixture.set(
        X_SHAPE_KIND_BOUNDING,
        &[Rect {
            x: 0,
            y: 0,
            width: 5,
            height: 10,
        }],
    );
    assert_eq!(ShapeFixture::error_of(&result), None);
    assert!(
        result.response.is_some(),
        "a shape change republishes the window rather than waiting for a draw"
    );

    // The masked pixels live in the presentation buffer the update carries,
    // which is what actually ships, rather than in the window's own backing.
    let updates = fixture.runtime.take_cpu_buffer_updates();
    let bytes = shaped_presentation_bytes(&updates).expect("a shaped presentation shipped");
    // Row zero: five opaque pixels, then five cleared ones.
    for x in 0..5 {
        assert_eq!(bytes[x * 4 + 3], 0xff, "pixel {x} is inside the shape");
    }
    for x in 5..10 {
        assert_eq!(
            &bytes[x * 4..x * 4 + 4],
            &[0, 0, 0, 0],
            "pixel {x} is outside the shape, and transparent rather than black"
        );
    }
}

/// The bytes and format of the last shaped presentation an update list
/// carries, so a test can assert on what actually ships.
fn shaped_presentation_bytes(updates: &[XAuthorityCpuBufferUpdate]) -> Option<Vec<u8>> {
    updates.iter().rev().find_map(|update| match update {
        XAuthorityCpuBufferUpdate::Replace(snapshot)
            if snapshot.format == X_AUTHORITY_CPU_BUFFER_FORMAT_ARGB8888 =>
        {
            Some(snapshot.bytes.to_vec())
        }
        XAuthorityCpuBufferUpdate::PatchBatch(batch)
            if batch.format == X_AUTHORITY_CPU_BUFFER_FORMAT_ARGB8888 =>
        {
            batch.patches.first().map(|patch| patch.bytes.clone())
        }
        _ => None,
    })
}

/// Shaping and unshaping moves the published buffer between the alpha and
/// opaque formats.
///
/// The transport shape is the store's business -- it replaces the whole
/// buffer when the format moves, which `crossing_the_format_boundary_ships_a_whole_buffer`
/// pins directly. What matters here is that the format a client's shape
/// request produces reaches the update that ships.
#[test]
fn crossing_between_shaped_and_unshaped_moves_the_published_format() {
    let mut fixture = ShapeFixture::new();
    let gc = 0x0020_0421;
    let create_gc = create_gc_request(ShapeFixture::ORDER, gc, ShapeFixture::WINDOW);
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create_gc)), None);
    let fill = poly_fill_rectangle_request(
        ShapeFixture::ORDER,
        ShapeFixture::WINDOW,
        gc,
        &[(0, 0, 10, 10)],
    );
    assert_eq!(ShapeFixture::error_of(&fixture.send(&fill)), None);

    let shaped = fixture.set(
        X_SHAPE_KIND_BOUNDING,
        &[Rect {
            x: 0,
            y: 0,
            width: 5,
            height: 10,
        }],
    );
    assert!(shaped.response.is_some());
    let updates = fixture.runtime.take_cpu_buffer_updates();
    let shaped_update = updates
        .iter()
        .find(|update| update.format() == X_AUTHORITY_CPU_BUFFER_FORMAT_ARGB8888)
        .expect("a shaped window publishes in the alpha format");
    // The whole buffer has to cross the transition, not a patch of it. Every
    // pixel already in the buffer was written under the opaque format, and
    // one left uncovered would be read as though it carried alpha.
    assert_eq!(
        shaped_update.size(),
        Size {
            width: 10,
            height: 10
        },
        "the transition covers the whole buffer"
    );

    // And back: unshaping returns the buffer to the opaque format.
    let unshaped = fixture.send(&shape_mask_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        0,
        0,
        0,
    ));
    assert!(unshaped.response.is_some());
    let updates = fixture.runtime.take_cpu_buffer_updates();
    assert!(
        updates
            .iter()
            .any(|update| update.format() == X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888),
        "unshaping publishes in the opaque format again: {updates:?}"
    );
}

/// A window nobody has drawn into is not republished.
///
/// There is no presentation buffer to reshape, and inventing one would ship
/// a frame for a window that has never had content.
#[test]
fn shaping_a_window_that_has_never_drawn_publishes_nothing() {
    let mut fixture = ShapeFixture::new();
    let result = fixture.set(
        X_SHAPE_KIND_BOUNDING,
        &[Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        }],
    );
    assert_eq!(ShapeFixture::error_of(&result), None);
    assert!(
        ShapeFixture::notify_of(&result).is_some(),
        "the shape still changed, and subscribers are still told"
    );
    assert!(
        result.response.is_none(),
        "but there is no frame to republish"
    );
}

impl RenderFixture {
    /// Compose the source picture over the destination with no mask, so a
    /// test can read what the source's transform and filter produced.
    fn composite_source_over(&mut self, width: u16, height: u16) -> XDispatchResult {
        self.send(&render_composite_request(
            Self::ORDER,
            1,
            Self::SOURCE_PICTURE,
            0,
            Self::PICTURE,
            0,
            0,
            0,
            0,
            0,
            0,
            width,
            height,
        ))
    }
}

/// A picture transform moves where a composite reads its source from.
///
/// RENDER's matrix maps a destination-relative coordinate to the source
/// pixel, so it is applied forward. GTK sends these at startup, which is why
/// refusing them ended both toolkits before they drew anything.
#[test]
fn render_picture_transforms_move_where_a_composite_samples() {
    // A two-pixel source: red then green, so which pixel was sampled is
    // visible in the result.
    let build = |transform: [i32; 9]| -> Vec<[u8; 4]> {
        let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
        fixture.add_source(2, 1, false);
        fixture.fill_source(
            [0xffff, 0, 0, 0xffff],
            Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        fixture.fill_source(
            [0, 0xffff, 0, 0xffff],
            Rect {
                x: 1,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        let set = fixture.send(&render_set_picture_transform_request(
            RenderFixture::ORDER,
            RenderFixture::SOURCE_PICTURE,
            transform,
        ));
        assert_eq!(RenderFixture::error_of(&set), None);
        assert_eq!(
            RenderFixture::error_of(&fixture.composite_source_over(4, 1)),
            None
        );
        (0..4).map(|x| fixture.pixel(x, 0)).collect()
    };

    const RED: [u8; 4] = [0, 0, 0xff, 0xff];
    const GREEN: [u8; 4] = [0, 0xff, 0, 0xff];
    const NOTHING: [u8; 4] = [0, 0, 0, 0];

    // Identity samples one-to-one, and the source is only two wide, so the
    // pixels beyond it read as transparent.
    assert_eq!(
        build(X_RENDER_IDENTITY_TRANSFORM),
        vec![RED, GREEN, NOTHING, NOTHING],
        "identity"
    );

    // Translation by one source pixel shifts the read left.
    let translate = [65536, 0, 65536, 0, 65536, 0, 0, 0, 65536];
    assert_eq!(
        build(translate),
        vec![GREEN, NOTHING, NOTHING, NOTHING],
        "translated"
    );

    // A constant divisor scales: with w = 2 every coordinate halves, so the
    // two source pixels each cover two destination pixels. This is the shape
    // conformance suites use for transform coverage, and it falls out of the
    // homogeneous divide rather than needing a special case.
    let half = [65536, 0, 0, 0, 65536, 0, 0, 0, 131072];
    assert_eq!(build(half), vec![RED, RED, GREEN, GREEN], "half scale");
}

/// A projective transform that sends a point to infinity has no source pixel
/// there, and the protocol's answer for no source is transparent black.
#[test]
fn render_projective_transforms_answer_nothing_where_they_diverge() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
    fixture.add_source(4, 1, true);
    fixture.fill_source(
        [0xffff, 0xffff, 0xffff, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 1,
        },
    );
    // w = x - 1.5, which is zero at the centre of destination pixel 1.
    let diverging = [65536, 0, 0, 0, 65536, 0, 65536, 0, -98304];
    let set = fixture.send(&render_set_picture_transform_request(
        RenderFixture::ORDER,
        RenderFixture::SOURCE_PICTURE,
        diverging,
    ));
    assert_eq!(RenderFixture::error_of(&set), None);
    assert_eq!(
        RenderFixture::error_of(&fixture.composite_source_over(4, 1)),
        None
    );
    assert_eq!(
        fixture.pixel(1, 0),
        [0, 0, 0, 0],
        "a point with no source samples transparent rather than panicking"
    );
}

/// Bilinear filtering blends the pixels either side of the sample point.
#[test]
fn render_bilinear_filtering_blends_between_source_pixels() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
    fixture.add_source(2, 1, false);
    // Black then white, so the blend is readable as a midpoint grey.
    fixture.fill_source(
        [0, 0, 0, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    fixture.fill_source(
        [0xffff, 0xffff, 0xffff, 0xffff],
        Rect {
            x: 1,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    let filter = fixture.send(&render_set_picture_filter_request(
        RenderFixture::ORDER,
        RenderFixture::SOURCE_PICTURE,
        "bilinear",
        &[],
    ));
    assert_eq!(RenderFixture::error_of(&filter), None);
    // Half scale, so destination pixel 1 samples exactly halfway between the
    // two source pixels: (1.5)/2 = 0.75, minus the half-pixel tap offset is
    // 0.25 into the span from pixel 0 to pixel 1.
    let half = [65536, 0, 0, 0, 65536, 0, 0, 0, 131072];
    let set = fixture.send(&render_set_picture_transform_request(
        RenderFixture::ORDER,
        RenderFixture::SOURCE_PICTURE,
        half,
    ));
    assert_eq!(RenderFixture::error_of(&set), None);
    assert_eq!(
        RenderFixture::error_of(&fixture.composite_source_over(4, 1)),
        None
    );
    let blended = fixture.pixel(1, 0);
    assert!(
        blended[0] > 0 && blended[0] < 0xff,
        "a bilinear tap between black and white is neither: {blended:?}"
    );
    // Nearest would have produced one or the other exactly.
    assert_ne!(blended[0], 0);
    assert_ne!(blended[0], 0xff);
}

/// The filter table is what a client reads before deciding what to ask for,
/// so its bytes are pinned.
///
/// Aliases come first on the wire, one slot per name, carrying the index of
/// the name each resolves to. `convolution` is deliberately absent: a client
/// that finds it missing disables its own kernel work cleanly.
#[test]
fn render_query_filters_answers_the_filters_this_server_honours() {
    let mut fixture = RenderFixture::new();
    let result = fixture.send(&render_query_filters_request(
        RenderFixture::ORDER,
        X_SETUP_DEFAULT_ROOT,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    let encoded = result.encoded_outputs(RenderFixture::ORDER);
    let reply = &encoded[0];
    assert_eq!(reply.len(), 76, "five aliases, five names, both padded");
    assert_eq!(read_u32(RenderFixture::ORDER, &reply[8..12]), 5, "aliases");
    assert_eq!(read_u32(RenderFixture::ORDER, &reply[12..16]), 5, "names");
    // Canonical entries carry no alias; the three aliases point at the names
    // they resolve to.
    let aliases: Vec<u16> = (0..5)
        .map(|i| read_u16(RenderFixture::ORDER, &reply[32 + i * 2..34 + i * 2]))
        .collect();
    assert_eq!(aliases, vec![0xffff, 0xffff, 0, 1, 1]);
    // Names follow the padded alias list.
    let names = &reply[44..76];
    let mut offset = 0;
    let mut read = Vec::new();
    while offset < names.len() {
        let len = usize::from(names[offset]);
        if len == 0 {
            break;
        }
        read.push(String::from_utf8_lossy(&names[offset + 1..offset + 1 + len]).to_string());
        offset += 1 + len;
    }
    assert_eq!(read, vec!["nearest", "bilinear", "fast", "good", "best"]);

    // A drawable that does not exist is refused rather than answered.
    let refused = fixture.send(&render_query_filters_request(
        RenderFixture::ORDER,
        0x0020_09ff,
    ));
    assert_eq!(
        RenderFixture::error_of(&refused),
        Some(XErrorCode::BadDrawable)
    );
}

/// Filter names are accepted, aliased, or refused on the terms the protocol
/// sets.
#[test]
fn render_set_picture_filter_accepts_what_it_advertises_and_refuses_the_rest() {
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    for name in ["nearest", "bilinear", "fast", "good", "best"] {
        let result = fixture.send(&render_set_picture_filter_request(
            RenderFixture::ORDER,
            RenderFixture::PICTURE,
            name,
            &[],
        ));
        assert_eq!(RenderFixture::error_of(&result), None, "filter {name}");
    }
    // Not advertised, so not a filter this server has.
    let convolution = fixture.send(&render_set_picture_filter_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        "convolution",
        &[],
    ));
    assert_eq!(
        RenderFixture::error_of(&convolution),
        Some(XErrorCode::BadValue)
    );
    // A filter this server does have, sent with parameters it does not take,
    // is a mismatch between the request and its argument rather than a bad
    // name.
    let with_params = fixture.send(&render_set_picture_filter_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        "nearest",
        &[65536],
    ));
    assert_eq!(
        RenderFixture::error_of(&with_params),
        Some(XErrorCode::BadMatch)
    );
}

impl RenderFixture {
    /// An opaque white source, so a trapezoid's coverage byte becomes the
    /// destination pixel and can be read directly.
    fn with_white_source(width: u16, height: u16) -> Self {
        let mut fixture = Self::with_argb_pixmap(width, height);
        fixture.add_source(width, height, true);
        fixture.fill_source(
            [0xffff, 0xffff, 0xffff, 0xffff],
            Rect {
                x: 0,
                y: 0,
                width: i32::from(width),
                height: i32::from(height),
            },
        );
        fixture
    }
}

/// A trapezoid covers what it encloses and nothing beyond it.
///
/// Ported from yserver's rasteriser tests. Compositing an opaque white
/// source through the coverage makes each destination pixel equal to the
/// coverage byte, so the assertions read the rasteriser directly.
#[test]
fn render_trapezoids_cover_their_interior_and_leave_the_rest_alone() {
    let mut fixture = RenderFixture::with_white_source(8, 8);
    // An axis-aligned box from (1,1) to (5,5).
    let result = fixture.send(&render_trapezoids_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        &[(
            fixed(1),
            fixed(5),
            (fixed(1), fixed(1)),
            (fixed(1), fixed(5)),
            (fixed(5), fixed(1)),
            (fixed(5), fixed(5)),
        )],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(
        fixture.pixel(2, 2),
        [0xff, 0xff, 0xff, 0xff],
        "the interior is fully covered"
    );
    assert_eq!(
        fixture.pixel(0, 0),
        [0, 0, 0, 0],
        "outside the trapezoid nothing was drawn"
    );
    assert_eq!(fixture.pixel(6, 6), [0, 0, 0, 0], "and beyond it");
}

/// A trapezoid that is not axis-aligned covers only its slant.
#[test]
fn render_trapezoids_narrow_with_their_slanted_sides() {
    let mut fixture = RenderFixture::with_white_source(16, 8);
    // A wedge: wide at the top, narrow at the bottom.
    let result = fixture.send(&render_trapezoids_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        &[(
            fixed(0),
            fixed(8),
            (fixed(0), fixed(0)),
            (fixed(6), fixed(8)),
            (fixed(16), fixed(0)),
            (fixed(10), fixed(8)),
        )],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    // The top row spans the full width; the bottom row does not.
    assert_ne!(fixture.pixel(1, 0)[3], 0, "wide at the top");
    assert_eq!(
        fixture.pixel(1, 7)[3],
        0,
        "the left slant has moved inward by the bottom"
    );
    assert_ne!(fixture.pixel(8, 7)[3], 0, "and the middle is still covered");
}

/// A triangle covers its interior, and a fan expands to the same triangles
/// an explicit list would have drawn.
#[test]
fn render_triangles_and_fans_cover_the_same_shapes() {
    let explicit = {
        let mut fixture = RenderFixture::with_white_source(8, 8);
        let result = fixture.send(&render_triangles_request(
            RenderFixture::ORDER,
            X_RENDER_TRIANGLES_MINOR_OPCODE,
            3,
            RenderFixture::SOURCE_PICTURE,
            RenderFixture::PICTURE,
            &[
                (fixed(0), fixed(0)),
                (fixed(6), fixed(0)),
                (fixed(0), fixed(6)),
            ],
        ));
        assert_eq!(RenderFixture::error_of(&result), None);
        assert_ne!(fixture.pixel(1, 1)[3], 0, "inside the triangle");
        assert_eq!(fixture.pixel(6, 6), [0, 0, 0, 0], "outside it");
        (0..8)
            .flat_map(|y| (0..8).map(move |x| (x, y)))
            .map(|(x, y)| fixture.pixel(x, y))
            .collect::<Vec<_>>()
    };

    // The same triangle as a fan of three points.
    let fanned = {
        let mut fixture = RenderFixture::with_white_source(8, 8);
        let result = fixture.send(&render_triangles_request(
            RenderFixture::ORDER,
            X_RENDER_TRI_FAN_MINOR_OPCODE,
            3,
            RenderFixture::SOURCE_PICTURE,
            RenderFixture::PICTURE,
            &[
                (fixed(0), fixed(0)),
                (fixed(6), fixed(0)),
                (fixed(0), fixed(6)),
            ],
        ));
        assert_eq!(RenderFixture::error_of(&result), None);
        (0..8)
            .flat_map(|y| (0..8).map(move |x| (x, y)))
            .map(|(x, y)| fixture.pixel(x, y))
            .collect::<Vec<_>>()
    };
    assert_eq!(explicit, fanned, "a fan draws the triangles it describes");
}

/// The source is anchored at the first primitive's leading corner.
///
/// Xorg's `fbTrapezoids` subtracts the first trapezoid's leading corner from
/// the source offset before compositing, so a client that measures that
/// offset from the shape's own corner reads the source it meant. Without the subtraction the read is
/// displaced by wherever the shape happens to sit, which is how a window
/// shadow ends up with a transparent band.
#[test]
fn render_trapezoids_anchor_the_source_at_the_first_corner() {
    // A source opaque in exactly one pixel, at (2, 2).
    let build = |source_x: i16, source_y: i16| -> [u8; 4] {
        let mut fixture = RenderFixture::with_argb_pixmap(8, 8);
        fixture.add_source(8, 8, false);
        fixture.fill_source(
            [0xffff, 0xffff, 0xffff, 0xffff],
            Rect {
                x: 2,
                y: 2,
                width: 1,
                height: 1,
            },
        );
        let result = fixture.send(&render_trapezoids_request(
            RenderFixture::ORDER,
            3,
            RenderFixture::SOURCE_PICTURE,
            RenderFixture::PICTURE,
            0,
            source_x,
            source_y,
            &[(
                fixed(2),
                fixed(4),
                (fixed(2), fixed(2)),
                (fixed(2), fixed(4)),
                (fixed(4), fixed(2)),
                (fixed(4), fixed(4)),
            )],
        ));
        assert_eq!(RenderFixture::error_of(&result), None);
        fixture.pixel(2, 2)
    };

    // Offset measured from the shape's corner: the anchor cancels it, the
    // read lands at the shape's own position, and the opaque pixel is found.
    assert_ne!(
        build(2, 2)[3],
        0,
        "an offset measured from the corner reads the source the client meant"
    );
    // A different offset reads somewhere else, which is what makes the
    // subtraction observable rather than decorative.
    assert_eq!(
        build(0, 0)[3],
        0,
        "and a different offset reads a different part of the source"
    );
}

/// Coverage requests refuse the operators and formats they must.
#[test]
fn render_coverage_requests_refuse_unimplemented_operators_and_formats() {
    let mut fixture = RenderFixture::with_white_source(4, 4);
    let trap = (
        fixed(0),
        fixed(2),
        (fixed(0), fixed(0)),
        (fixed(0), fixed(2)),
        (fixed(2), fixed(0)),
        (fixed(2), fixed(2)),
    );
    // An operator the protocol defines and this server withholds.
    let withheld = fixture.send(&render_trapezoids_request(
        RenderFixture::ORDER,
        0x13,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        &[trap],
    ));
    assert_eq!(
        RenderFixture::error_of(&withheld),
        Some(XErrorCode::BadImplementation)
    );
    // A mask format naming nothing this server has.
    let bad_format = fixture.send(&render_trapezoids_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        0x1234,
        0,
        0,
        &[trap],
    ));
    assert_eq!(
        RenderFixture::error_of(&bad_format),
        Some(XErrorCode::RenderPictFormat)
    );
    // An empty list is accepted and draws nothing.
    let empty = fixture.send(&render_trapezoids_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        &[],
    ));
    assert_eq!(RenderFixture::error_of(&empty), None);
}

/// Glyphs composited onto a window reach the screen.
///
/// Every other glyph test here draws into a pixmap, which is not what a
/// toolkit does with text it wants visible: GTK composites glyphs straight
/// onto the window. That path goes through the drawing funnel rather than
/// ending in the store, and nothing had exercised it.
#[test]
fn render_composite_glyphs_onto_a_window_reaches_the_drawing_funnel() {
    let window = 0x0020_0500;
    let picture = 0x0020_0501;
    let glyphset = 0x0020_0502;
    let mut fixture = RenderFixture::new();

    let create = create_window_request(RenderFixture::ORDER, window, 0, 0, 16, 16);
    assert_eq!(RenderFixture::error_of(&fixture.send(&create)), None);
    let bind = render_create_picture_request(
        RenderFixture::ORDER,
        picture,
        window,
        X_RENDER_FORMAT_RGB24,
        &[],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&bind)), None);

    // A one-pixel repeating source, the way a client paints text one colour.
    fixture.add_source(1, 1, true);
    fixture.fill_source(
        [0xffff, 0xffff, 0xffff, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );

    let set = render_create_glyph_set_request(RenderFixture::ORDER, glyphset, X_RENDER_FORMAT_A8);
    assert_eq!(RenderFixture::error_of(&fixture.send(&set)), None);
    let add = render_add_glyphs_request(
        RenderFixture::ORDER,
        glyphset,
        &[(1, [2, 1], [0, 0, 2, 0], vec![0xff, 0xff, 0, 0])],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&add)), None);

    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        picture,
        X_RENDER_FORMAT_A8,
        glyphset,
        0,
        0,
        (1, 1),
        &[1],
    ));
    assert_eq!(
        RenderFixture::error_of(&result),
        None,
        "compositing glyphs onto a window must not be refused"
    );
}

/// A picture outlives the pixmap it was made from, and still takes glyphs.
///
/// A toolkit frees the pixmap as soon as it has a picture for it and keeps
/// drawing through the picture. The backing is retained for exactly that
/// reason, and text is what it is usually retained for.
#[test]
fn render_composite_glyphs_into_a_retained_pixmap_backing() {
    let glyphset = 0x0020_0512;
    let mut fixture = RenderFixture::with_argb_pixmap(8, 8);
    fixture.add_source(1, 1, true);
    fixture.fill_source(
        [0xffff, 0xffff, 0xffff, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    let set = render_create_glyph_set_request(RenderFixture::ORDER, glyphset, X_RENDER_FORMAT_A8);
    assert_eq!(RenderFixture::error_of(&fixture.send(&set)), None);
    let add = render_add_glyphs_request(
        RenderFixture::ORDER,
        glyphset,
        &[(1, [2, 1], [0, 0, 2, 0], vec![0xff, 0xff, 0, 0])],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&add)), None);

    // The pixmap goes away; the picture stays.
    let free = fixture.send(&free_pixmap_request(
        RenderFixture::ORDER,
        RenderFixture::PIXMAP,
    ));
    assert_eq!(RenderFixture::error_of(&free), None);

    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        glyphset,
        0,
        0,
        (1, 1),
        &[1],
    ));
    assert_eq!(
        RenderFixture::error_of(&result),
        None,
        "a picture whose pixmap was freed still draws"
    );
}

/// A glyph that falls outside the destination does not refuse the run.
///
/// Text is positioned by a pen that walks along a line, and a client draws
/// runs that extend past the edge of what it is drawing into -- a label
/// wider than its window, or text scrolled partly out of view. The glyphs
/// that miss simply do not appear; refusing the request would take the
/// visible ones with them.
#[test]
fn render_composite_glyphs_outside_the_destination_still_draw_the_rest() {
    let glyphset = 0x0020_0520;
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    fixture.add_source(1, 1, true);
    fixture.fill_source(
        [0xffff, 0xffff, 0xffff, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    let set = render_create_glyph_set_request(RenderFixture::ORDER, glyphset, X_RENDER_FORMAT_A8);
    assert_eq!(RenderFixture::error_of(&fixture.send(&set)), None);
    // A two-pixel-wide glyph advancing two pixels each time.
    let add = render_add_glyphs_request(
        RenderFixture::ORDER,
        glyphset,
        &[(1, [2, 1], [0, 0, 2, 0], vec![0xff, 0xff, 0, 0])],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&add)), None);

    // Six of them from x=0 runs off the right edge of a four-wide pixmap.
    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        glyphset,
        0,
        0,
        (0, 0),
        &[1, 1, 1, 1, 1, 1],
    ));
    assert_eq!(
        RenderFixture::error_of(&result),
        None,
        "a run that overruns the destination must not be refused"
    );
    assert_ne!(
        fixture.pixel(0, 0)[3],
        0,
        "and the glyphs that did land are drawn"
    );
}

/// A composite clipped by the destination edge still reads the source it
/// would have read unclipped.
///
/// Clipping moves where the destination walk begins, so the source read has
/// to move with it. Getting this wrong draws the wrong part of the source
/// along every edge, which is the kind of fault that looks like a rendering
/// glitch rather than a bug.
#[test]
fn render_composite_clipped_by_the_edge_reads_the_same_source_pixels() {
    // A four-wide source whose third pixel is the distinctive one.
    let build = |destination_x: i16| -> [u8; 4] {
        let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
        fixture.add_source(4, 1, false);
        fixture.fill_source(
            [0, 0xffff, 0, 0xffff],
            Rect {
                x: 2,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        let result = fixture.send(&render_composite_request(
            RenderFixture::ORDER,
            1,
            RenderFixture::SOURCE_PICTURE,
            0,
            RenderFixture::PICTURE,
            0,
            0,
            0,
            0,
            destination_x,
            0,
            4,
            1,
        ));
        assert_eq!(RenderFixture::error_of(&result), None);
        // Destination pixel 2 corresponds to source pixel 2 only when the
        // composite starts at the origin; started at -2 it shows source 4,
        // which does not exist. What matters is that starting at 0 works and
        // the clipped case does not shift the reads.
        fixture.pixel(2, 0)
    };

    // Unclipped: destination 2 reads source 2, the green pixel.
    assert_eq!(build(0), [0, 0xff, 0, 0xff], "unclipped");

    // Starting left of the drawable clips the first two columns away. The
    // remaining destination pixels must still read the source pixels they
    // were always going to read: destination 2 now corresponds to source 4,
    // which is past the end of a non-repeating source, so it is transparent
    // rather than the green pixel wrongly shifted into view.
    assert_eq!(build(-2), [0, 0, 0, 0], "clipped, and not shifted");
}

/// A solid fill is a source of one colour with no drawable behind it.
#[test]
fn render_solid_fills_paint_the_colour_they_were_given() {
    let solid = 0x0020_0600;
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    // Opaque red, premultiplied on the wire as the protocol defines.
    let create = render_create_solid_fill_request(
        RenderFixture::ORDER,
        solid,
        [0xffff, 0, 0, 0xffff],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&create)), None);
    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        solid,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        0,
        0,
        2,
        2,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(0, 0), [0, 0, 0xff, 0xff]);
    assert_eq!(fixture.pixel(1, 1), [0, 0, 0xff, 0xff], "everywhere");
}

/// A linear gradient runs between its stops.
///
/// Cairo paints widget backgrounds with these and sends them without asking
/// what version the server offers, which is how they arrived here.
#[test]
fn render_linear_gradients_run_between_their_stops() {
    let gradient = 0x0020_0601;
    let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
    // Black at the left edge, white at the right.
    let create = render_create_linear_gradient_request(
        RenderFixture::ORDER,
        gradient,
        (0, 0),
        (4 * 65536, 0),
        &[
            (0, [0, 0, 0, 0xffff]),
            (65536, [0xffff, 0xffff, 0xffff, 0xffff]),
        ],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&create)), None);
    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        gradient,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        0,
        0,
        4,
        1,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);

    let ramp: Vec<u8> = (0..4).map(|x| fixture.pixel(x, 0)[2]).collect();
    assert!(
        ramp.windows(2).all(|pair| pair[0] < pair[1]),
        "the gradient rises from one stop to the other: {ramp:?}"
    );
    assert!(ramp[0] < 0x40, "dark at the first stop: {ramp:?}");
    assert!(ramp[3] > 0xb0, "light at the last: {ramp:?}");
    // Every pixel is opaque: both stops are, and interpolating in straight
    // alpha is what keeps the middle from dimming.
    for x in 0..4 {
        assert_eq!(fixture.pixel(x, 0)[3], 0xff, "opaque at {x}");
    }
}

/// A gradient with no stops has no colour to show, and is refused.
#[test]
fn render_gradients_without_stops_are_refused() {
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    let create = render_create_linear_gradient_request(
        RenderFixture::ORDER,
        0x0020_0602,
        (0, 0),
        (65536, 0),
        &[],
    );
    assert_eq!(
        RenderFixture::error_of(&fixture.send(&create)),
        Some(XErrorCode::BadValue)
    );
}

/// Pad is what a client painting a gradient reaches for, and it holds the
/// end colours rather than repeating or clipping them.
#[test]
fn render_pad_repeat_holds_a_gradients_end_colours() {
    let gradient = 0x0020_0603;
    let mut fixture = RenderFixture::with_argb_pixmap(6, 1);
    // The ramp is defined over the first two pixels only.
    let create = render_create_linear_gradient_request(
        RenderFixture::ORDER,
        gradient,
        (0, 0),
        (2 * 65536, 0),
        &[
            (0, [0, 0, 0, 0xffff]),
            (65536, [0xffff, 0xffff, 0xffff, 0xffff]),
        ],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&create)), None);
    let pad = render_change_picture_request(RenderFixture::ORDER, gradient, &[(0, 2)]);
    assert_eq!(RenderFixture::error_of(&fixture.send(&pad)), None);
    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        gradient,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        0,
        0,
        6,
        1,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    // Past the end of the ramp the last stop's colour is held.
    assert_eq!(fixture.pixel(5, 0), [0xff, 0xff, 0xff, 0xff], "padded");
}

/// A point translated between two windows lands where it actually is.
///
/// A toolkit positions a dropdown by asking where its parent window sits on
/// screen and offsetting the popup from there. Answering with the point it
/// was handed tells every client its window is at the screen origin, so the
/// menu opens wherever the window is not -- offset by exactly the window's
/// position, which on a second monitor is most of a screen away.
#[test]
fn translate_coordinates_moves_a_point_between_window_spaces() {
    let parent = 0x0020_0700;
    let child = 0x0020_0701;
    let mut fixture = RenderFixture::new();

    let create = create_window_request(RenderFixture::ORDER, parent, 2553, 41, 1258, 1390);
    assert_eq!(RenderFixture::error_of(&fixture.send(&create)), None);
    let nested = create_window_request_with_parent(
        RenderFixture::ORDER,
        child,
        parent,
        10,
        20,
        100,
        50,
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&nested)), None);

    let translate = |fixture: &mut RenderFixture, source, destination, x, y| {
        let request =
            translate_coordinates_request(RenderFixture::ORDER, source, destination, x, y);
        match fixture.send(&request).outputs.as_slice() {
            [XClientOutput::Reply(XClientReply::TranslateCoordinates { dst_x, dst_y, .. })] => {
                (*dst_x, *dst_y)
            }
            other => panic!("TranslateCoordinates produced {other:?}"),
        }
    };

    // The window's own origin, in root coordinates: where it actually is.
    assert_eq!(
        translate(&mut fixture, parent, X_SETUP_DEFAULT_ROOT, 0, 0),
        (2553, 41),
        "a toplevel's origin is its position, not the screen origin"
    );
    // A point inside it carries the same offset.
    assert_eq!(
        translate(&mut fixture, parent, X_SETUP_DEFAULT_ROOT, 100, 200),
        (2653, 241)
    );
    // A nested window accumulates its ancestors' offsets.
    assert_eq!(
        translate(&mut fixture, child, X_SETUP_DEFAULT_ROOT, 0, 0),
        (2563, 61),
        "a child's root position includes every parent between it and the root"
    );
    // And back the other way.
    assert_eq!(
        translate(&mut fixture, X_SETUP_DEFAULT_ROOT, child, 2563, 61),
        (0, 0)
    );
    // Between siblings in the same space, the translation is the difference.
    assert_eq!(
        translate(&mut fixture, child, parent, 0, 0),
        (10, 20),
        "a child's origin in its parent's space is its own geometry"
    );
    // The identity case still holds.
    assert_eq!(translate(&mut fixture, parent, parent, 7, 9), (7, 9));
}

/// The FetchRegion reply is read by a client at fixed offsets, so its bytes
/// are pinned.
///
/// The reply carries extents at bytes 8 through 16 and no rectangle count at
/// all -- a client derives the count from the reply's length. This encoder
/// previously wrote a count where the extents' x belongs, so every client
/// read the number of rectangles as a coordinate and zero for the rest of the
/// bounding box. The test that existed destructured the reply before it was
/// encoded and could not see it.
#[test]
fn xfixes_fetch_region_reply_matches_the_bytes_a_client_reads() {
    let mut fixture = XfixesRegionFixture::new();
    fixture.create(
        XfixesRegionFixture::A,
        &[
            Rect {
                x: 3,
                y: 4,
                width: 5,
                height: 6,
            },
            Rect {
                x: 20,
                y: 30,
                width: 2,
                height: 2,
            },
        ],
    );
    let request = xfixes_fetch_region_request(XfixesRegionFixture::ORDER, XfixesRegionFixture::A);
    let result = fixture.send(&request);
    let encoded = result.encoded_outputs(XfixesRegionFixture::ORDER);
    let reply = &encoded[0];

    // Two rectangles: a 32-byte header and two eight-byte rectangles, with the
    // length counting only what follows the header, in four-byte units.
    assert_eq!(reply.len(), 32 + 2 * 8);
    assert_eq!(reply[0], 1, "a reply, not an event");
    assert_eq!(read_u32(XfixesRegionFixture::ORDER, &reply[4..8]), 4);

    // Extents, at the offsets the protocol puts them.
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[8..10]), 3, "x");
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[10..12]), 4, "y");
    assert_eq!(
        read_u16(XfixesRegionFixture::ORDER, &reply[12..14]),
        19,
        "width spans both rectangles"
    );
    assert_eq!(
        read_u16(XfixesRegionFixture::ORDER, &reply[14..16]),
        28,
        "height spans both rectangles"
    );
    // The rest of the header is padding, and must not carry a count.
    assert!(
        reply[16..32].iter().all(|byte| *byte == 0),
        "bytes 16 through 32 are padding: {:?}",
        &reply[16..32]
    );

    // Then the rectangles themselves.
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[32..34]), 3);
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[34..36]), 4);
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[36..38]), 5);
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[38..40]), 6);
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[40..42]), 20);
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[42..44]), 30);
}

/// The version answered is the lower of the client's and this server's.
///
/// It answered its own version regardless of what was asked. A client that
/// asked for version 1 was told 6, and would then be entitled to send
/// requests no version 1 client should know about.
#[test]
fn xfixes_query_version_answers_the_lower_of_the_two() {
    let mut fixture = XfixesRegionFixture::new();
    let ask = |fixture: &mut XfixesRegionFixture, major: u32, minor: u32| {
        let mut out = vec![X_XFIXES_MAJOR_OPCODE, X_XFIXES_QUERY_VERSION_MINOR_OPCODE];
        push_u16(&mut out, XfixesRegionFixture::ORDER, 3);
        push_u32(&mut out, XfixesRegionFixture::ORDER, major);
        push_u32(&mut out, XfixesRegionFixture::ORDER, minor);
        let encoded = fixture.send(&out).encoded_outputs(XfixesRegionFixture::ORDER);
        (
            read_u32(XfixesRegionFixture::ORDER, &encoded[0][8..12]),
            read_u32(XfixesRegionFixture::ORDER, &encoded[0][12..16]),
        )
    };

    // A client below the server is answered its own version.
    assert_eq!(ask(&mut fixture, 2, 0), (2, 0));
    assert_eq!(ask(&mut fixture, 1, 0), (1, 0));
    // A client above the server is answered the server's.
    assert_eq!(ask(&mut fixture, 9, 0), (6, 0));
    // Equal majors take the lower minor.
    assert_eq!(ask(&mut fixture, 6, 0), (6, 0));
}

/// A region can be built from a window's shape, and Input is not one of the
/// shapes it may be built from.
#[test]
fn xfixes_builds_a_region_from_a_window_shape() {
    let window = 0x0020_0800;
    let mut fixture = XfixesRegionFixture::new();
    let create = create_window_request(XfixesRegionFixture::ORDER, window, 5, 7, 40, 30);
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&create)), None);

    // An unshaped window reports its own bounds.
    let from_window = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_WINDOW_MINOR_OPCODE,
        XfixesRegionFixture::A,
        window,
        X_XFIXES_WINDOW_REGION_BOUNDING,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&from_window), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::A),
        vec![Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 30,
        }],
        "an unshaped window's region is the window rectangle"
    );

    // XFIXES builds a region from the bounding or clip shape and no other.
    // SHAPE has a third kind and this request does not accept it.
    let input_kind = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_WINDOW_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        window,
        2,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&input_kind),
        Some(XErrorCode::BadValue)
    );

    // A window that does not exist is named before the kind is judged.
    let unknown = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_WINDOW_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        0x0020_08ff,
        2,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&unknown),
        Some(XErrorCode::BadWindow),
        "the missing window is reported before the bad kind"
    );
}

/// A region built from a graphics context copies that context's clip
/// rectangles as they are stored.
///
/// The clip origins describe how the graphics context *uses* its clip, not
/// where the rectangles are, so folding them in here would apply them a second
/// time if the region were installed as a clip again. Nonzero stored origins
/// must therefore leave the extracted region unchanged.
#[test]
fn xfixes_copies_a_graphics_context_clip() {
    let gc = 0x0020_0810;
    let pixmap = 0x0020_0811;
    let window = 0x0020_0812;
    let mut fixture = XfixesRegionFixture::new();
    let create_window = create_window_request(XfixesRegionFixture::ORDER, window, 0, 0, 32, 32);
    assert_eq!(
        XfixesRegionFixture::error_of(&fixture.send(&create_window)),
        None
    );
    let create_pixmap =
        create_pixmap_request(XfixesRegionFixture::ORDER, 24, pixmap, window, 32, 32);
    assert_eq!(
        XfixesRegionFixture::error_of(&fixture.send(&create_pixmap)),
        None
    );
    let create_gc = create_gc_request(XfixesRegionFixture::ORDER, gc, pixmap);
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&create_gc)), None);

    // A graphics context with no clip has nothing to copy, which is not the
    // same as the graphics context being absent.
    let no_clip = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_GC_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        gc,
        0,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&no_clip),
        Some(XErrorCode::BadMatch)
    );

    let mut clip = set_clip_rectangles_request(XfixesRegionFixture::ORDER, gc, &[(1, 2, 3, 4)]);
    clip[8..10].copy_from_slice(&(-5_i16).to_le_bytes());
    clip[10..12].copy_from_slice(&7_i16.to_le_bytes());
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&clip)), None);
    let from_gc = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_GC_MINOR_OPCODE,
        XfixesRegionFixture::A,
        gc,
        0,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&from_gc), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::A),
        vec![Rect {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        }],
        "the stored clip rectangles arrive unmoved"
    );

    // An absent graphics context is named as such.
    let unknown = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_GC_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        0x0020_08fe,
        0,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&unknown),
        Some(XErrorCode::BadGraphicsContext)
    );
}

/// Expanding grows every rectangle, and an empty source leaves the
/// destination alone.
#[test]
fn xfixes_expands_a_region_and_leaves_an_empty_one_alone() {
    let mut fixture = XfixesRegionFixture::new();
    fixture.create(
        XfixesRegionFixture::A,
        &[Rect {
            x: 10,
            y: 10,
            width: 4,
            height: 4,
        }],
    );
    fixture.create(XfixesRegionFixture::B, &[]);
    fixture.create(
        XfixesRegionFixture::OUT,
        &[Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }],
    );

    let expand = fixture.send(&xfixes_expand_region_request(
        XfixesRegionFixture::ORDER,
        XfixesRegionFixture::A,
        XfixesRegionFixture::OUT,
        1,
        2,
        3,
        4,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&expand), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 9,
            y: 7,
            width: 7,
            height: 11,
        }]
    );

    // An empty source leaves the destination as it was, rather than emptying
    // it -- the edge a reimplementation gets backwards.
    let empty = fixture.send(&xfixes_expand_region_request(
        XfixesRegionFixture::ORDER,
        XfixesRegionFixture::B,
        XfixesRegionFixture::OUT,
        5,
        5,
        5,
        5,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&empty), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 9,
            y: 7,
            width: 7,
            height: 11,
        }],
        "expanding nothing changes nothing"
    );

    // Rectangles that grow into each other merge. Expanding each one and
    // stopping there would answer with two overlapping rectangles, which is
    // not a region -- the result has to be unioned back into canonical form.
    let pair = 0x0020_0303;
    fixture.create(
        pair,
        &[
            Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            },
            Rect {
                x: 10,
                y: 0,
                width: 4,
                height: 4,
            },
        ],
    );
    let merged = fixture.send(&xfixes_expand_region_request(
        XfixesRegionFixture::ORDER,
        pair,
        XfixesRegionFixture::OUT,
        4,
        4,
        4,
        4,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&merged), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: -4,
            y: -4,
            width: 22,
            height: 12,
        }],
        "two grown rectangles that meet become one"
    );
}

/// Every XFIXES minor either fails to decode or is answered. None escapes to
/// the fallthrough.
///
/// A decoder with no dispatcher behind it is worse than no decoder at all: the
/// request decodes, misses every family matcher, and reaches
/// `unreachable!("extension request escaped its family dispatcher")`, which
/// takes the server down on input a client is free to send. That is not a
/// refusal, it is a crash, and it is reachable from any unprivileged client.
/// This sweep walks the whole minor range at each minor's real request length
/// so the decode-then-dispatch path actually runs for every one of them.
#[test]
fn every_xfixes_minor_is_answered_rather_than_escaping_dispatch() {
    // Minors whose decoders demand an exact length, sent at that length so
    // decoding succeeds and dispatch is genuinely exercised.
    let sized: &[(u8, usize)] = &[
        (X_XFIXES_CREATE_REGION_FROM_BITMAP_MINOR_OPCODE, 12),
        (X_XFIXES_CREATE_REGION_FROM_WINDOW_MINOR_OPCODE, 16),
        (X_XFIXES_CREATE_REGION_FROM_GC_MINOR_OPCODE, 12),
        (X_XFIXES_CREATE_REGION_FROM_PICTURE_MINOR_OPCODE, 12),
        (X_XFIXES_EXPAND_REGION_MINOR_OPCODE, 20),
    ];

    for minor in 0..=X_XFIXES_LAST_MINOR_OPCODE {
        let length = sized
            .iter()
            .find(|(opcode, _)| *opcode == minor)
            .map_or(4, |(_, length)| *length);
        let mut bytes = vec![X_XFIXES_MAJOR_OPCODE, minor];
        push_u16(&mut bytes, XfixesRegionFixture::ORDER, (length / 4) as u16);
        bytes.resize(length, 0);

        let mut fixture = XfixesRegionFixture::new();
        let decoded = decode_x11_core_request(
            context(XfixesRegionFixture::NS, 9000 + u64::from(minor), XfixesRegionFixture::ORDER),
            &bytes,
        );
        let Ok(request) = decoded else {
            // Refusing to decode is a fine answer; it becomes a wire error.
            continue;
        };
        // The assertion is that this returns at all. If the minor decoded
        // without a dispatcher, this call panics.
        let result = dispatch_x11_wire_request(
            dispatch_context(XfixesRegionFixture::NS, minor.into(), XfixesRegionFixture::ORDER, X_XFIXES_MAJOR_OPCODE),
            request,
            &mut fixture.runtime,
            &mut fixture.atoms,
            &mut fixture.properties,
        );
        assert!(
            !result.outputs.is_empty() || minor == X_XFIXES_DESTROY_REGION_MINOR_OPCODE,
            "minor {minor} produced no answer at all"
        );
    }
}

/// A region built from a depth-one bitmap is the bitmap's set bits.
///
/// This is minor 6, and it is the one a reimplementation is most likely to
/// accept and quietly answer with an empty region -- the client then draws
/// nothing and has no error to explain it.
#[test]
fn xfixes_builds_a_region_from_a_bitmaps_set_bits() {
    let mask = 0x0020_0820;
    let gc = 0x0020_0821;
    let mut fixture = XfixesRegionFixture::new();
    let create = create_pixmap_request(XfixesRegionFixture::ORDER, 1, mask, X_SETUP_DEFAULT_ROOT, 4, 2);
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&create)), None);
    let create_gc = create_gc_request(XfixesRegionFixture::ORDER, gc, mask);
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&create_gc)), None);
    // The two leftmost pixels of the first row, nothing on the second.
    let data: Vec<u8> = vec![0b0000_0011, 0, 0, 0, 0, 0, 0, 0];
    let put = put_image_request_at_depth(XfixesRegionFixture::ORDER, 1, mask, gc, 4, 2, &data);
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&put)), None);

    let from_bitmap = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_BITMAP_MINOR_OPCODE,
        XfixesRegionFixture::A,
        mask,
        0,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&from_bitmap), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::A),
        vec![Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        }],
        "the set bits, and not an empty region"
    );

    // A pixmap that is not one bit deep does not describe a region.
    let deep = 0x0020_0822;
    let create_deep =
        create_pixmap_request(XfixesRegionFixture::ORDER, 24, deep, X_SETUP_DEFAULT_ROOT, 4, 2);
    assert_eq!(
        XfixesRegionFixture::error_of(&fixture.send(&create_deep)),
        None
    );
    let too_deep = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_BITMAP_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        deep,
        0,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&too_deep),
        Some(XErrorCode::BadMatch)
    );

    // And a pixmap that does not exist is a bad pixmap, not a bad match.
    let absent = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_BITMAP_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        0x0020_08fd,
        0,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&absent),
        Some(XErrorCode::BadPixmap)
    );
}

/// The extension error bases must partition. A client adds an offset to a base
/// and has no other way to tell whose error it is holding.
#[test]
fn extension_error_bases_do_not_overlap_each_other_or_the_core_range() {
    // BadImplementation is the last core code; XI and RENDER define five codes
    // each, GLX fourteen.
    const CORE_LAST: u8 = 17;
    let spans: [(&str, u8, u8); 3] = [
        ("XI", X_INPUT_FIRST_ERROR, 5),
        ("RENDER", X_RENDER_FIRST_ERROR, 5),
        ("GLX", X_GLX_FIRST_ERROR, X_GLX_ERROR_COUNT),
    ];
    for (name, base, count) in spans {
        assert!(count > 0, "{name} reserves no codes");
        assert!(
            base > CORE_LAST,
            "{name} base {base} falls inside the core range",
        );
        assert!(
            base.checked_add(count - 1).is_some(),
            "{name} span runs past the end of the code space",
        );
    }
    for (index, (first_name, first_base, first_count)) in spans.iter().enumerate() {
        for (second_name, second_base, second_count) in spans.iter().skip(index + 1) {
            let first_end = first_base + first_count - 1;
            let second_end = second_base + second_count - 1;
            assert!(
                first_end < *second_base || second_end < *first_base,
                "{first_name} {first_base}..={first_end} overlaps \
                 {second_name} {second_base}..={second_end}",
            );
        }
    }
}

/// No two errors may share a wire code, and an extension error must sit inside
/// its own extension's span.
///
/// GLX answered from base zero until now, which put `GLXBadPixmap` on code 3 --
/// the core `BadWindow` -- so a refusal from one arrived as the other.
#[test]
fn every_wire_error_code_is_distinct_and_inside_its_extension_span() {
    let codes = [
        ("BadRequest", XErrorCode::BadRequest),
        ("BadValue", XErrorCode::BadValue),
        ("BadWindow", XErrorCode::BadWindow),
        ("BadPixmap", XErrorCode::BadPixmap),
        ("BadDrawable", XErrorCode::BadDrawable),
        ("BadAtom", XErrorCode::BadAtom),
        ("BadFont", XErrorCode::BadFont),
        ("BadMatch", XErrorCode::BadMatch),
        ("BadAccess", XErrorCode::BadAccess),
        ("BadAlloc", XErrorCode::BadAlloc),
        ("BadColor", XErrorCode::BadColor),
        ("BadGraphicsContext", XErrorCode::BadGraphicsContext),
        ("BadIdChoice", XErrorCode::BadIdChoice),
        ("BadName", XErrorCode::BadName),
        ("BadLength", XErrorCode::BadLength),
        ("BadImplementation", XErrorCode::BadImplementation),
        ("XiBadDevice", XErrorCode::XiBadDevice),
        ("RenderPictFormat", XErrorCode::RenderPictFormat),
        ("RenderPicture", XErrorCode::RenderPicture),
        ("RenderPictOp", XErrorCode::RenderPictOp),
        ("RenderGlyphSet", XErrorCode::RenderGlyphSet),
        ("RenderGlyph", XErrorCode::RenderGlyph),
        ("GlxBadDrawable", XErrorCode::GlxBadDrawable),
        ("GlxBadPixmap", XErrorCode::GlxBadPixmap),
        ("GlxBadFbConfig", XErrorCode::GlxBadFbConfig),
    ];
    for (index, (name, code)) in codes.iter().enumerate() {
        for (other_name, other) in codes.iter().skip(index + 1) {
            assert_ne!(
                code.wire_code(),
                other.wire_code(),
                "{name} and {other_name} both answer to code {}",
                code.wire_code(),
            );
        }
    }
    let glx_last = X_GLX_FIRST_ERROR + X_GLX_ERROR_COUNT - 1;
    for (name, code) in [
        ("GlxBadDrawable", XErrorCode::GlxBadDrawable),
        ("GlxBadPixmap", XErrorCode::GlxBadPixmap),
        ("GlxBadFbConfig", XErrorCode::GlxBadFbConfig),
    ] {
        let wire = code.wire_code();
        assert!(
            (X_GLX_FIRST_ERROR..=glx_last).contains(&wire),
            "{name} answers {wire}, outside GLX {X_GLX_FIRST_ERROR}..={glx_last}",
        );
    }
}

#[test]
fn xkb_level_names_are_real_atoms_because_a_client_may_ask_what_they_are_called() {
    let namespace = NamespaceId::from_raw(71);
    let order = XByteOrder::LittleEndian;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    // Exactly what libxdo asks a display on startup: key type names, key type
    // level names, virtual modifier names.
    let reply = dispatch_x11_wire_request(
        dispatch_context(namespace, 9, order, X_KEYBOARD_MAJOR_OPCODE),
        XWireRequest::XkbGetNames { which: 0x8c0 },
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(order);

    let body = &reply[0][32..];
    let types = usize::from(reply[0][14]);
    assert!(types > 0, "the reply must advertise key types");
    let type_atoms: Vec<u32> = (0..types)
        .map(|index| read_u32(order, &body[index * 4..index * 4 + 4]))
        .collect();
    // Type names, then one level-count byte per type padded to a word, then
    // the level names themselves: two per type, in type order.
    let levels_at = (types * 4 + types).div_ceil(4) * 4;
    let level_atoms: Vec<u32> = (0..types * 2)
        .map(|index| read_u32(order, &body[levels_at + index * 4..levels_at + index * 4 + 4]))
        .collect();

    assert_eq!(
        level_atoms.len(),
        type_atoms.len() * 2,
        "two named levels per type, matching the numLevels XkbGetMap advertises"
    );
    // NONE IS FATAL TO A CLIENT THAT ASKS. Every one of these used to be atom
    // None, which the spec permits for an unnamed level. libxdo walks the list
    // and asks the server what each name is; the server refuses None with
    // BadAtom, and libX11 exits a client whose request was refused. No real
    // server sends None here, so nothing ever met it there.
    for (index, atom) in type_atoms.iter().chain(&level_atoms).enumerate() {
        assert_ne!(*atom, 0, "name {index} is None and a client may ask for it");
    }
}

#[test]
fn xkb_latch_lock_state_is_answered_when_the_state_it_asks_for_already_holds() {
    let namespace = NamespaceId::from_raw(72);
    let order = XByteOrder::LittleEndian;
    // The request xdotool sends before every type: be on group zero, touch no
    // modifier. Byte for byte as libX11 puts it on the wire.
    let request = decode_x11_core_request(
        context(namespace, 4, order),
        &[
            X_KEYBOARD_MAJOR_OPCODE,
            X_KEYBOARD_LATCH_LOCK_STATE_MINOR_OPCODE,
            4,
            0,
            0,
            1, // deviceSpec: XkbUseCoreKbd
            0,
            0, // affectModLocks, modLocks
            1,
            0, // lockGroup, groupLock = 0
            0,
            0, // affectModLatches, modLatches
            0,
            0, // pad, latchGroup
            0,
            0, // groupLatch
        ],
    )
    .unwrap();
    assert_eq!(
        request,
        XWireRequest::XkbLatchLockState {
            affect_mod_locks: 0,
            mod_locks: 0,
            lock_group: true,
            group_lock: 0,
            affect_mod_latches: 0,
            mod_latches: 0,
            latch_group: false,
            group_latch: 0,
        }
    );

    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, order, X_KEYBOARD_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    // Nothing owed and nothing refused: one group means group zero is where
    // the keyboard already is. Answering BadRequest here, which is what an
    // undecoded minor does, is fatal to the client -- libX11 exits a client
    // whose request the server refuses, so xdotool died before typing a key.
    assert!(result.outputs.is_empty(), "{:?}", result.outputs);
    assert!(result.response.is_none());
}

#[test]
fn xkb_latch_lock_state_refuses_a_group_and_a_modifier_this_keymap_has_not_got() {
    let namespace = NamespaceId::from_raw(73);
    let order = XByteOrder::LittleEndian;
    let dispatch = |request| {
        let mut runtime = XAuthorityRuntime::new();
        let mut atoms = XAtomTable::new();
        let mut properties = XPropertyTable::new();
        dispatch_x11_wire_request(
            dispatch_context(namespace, 5, order, X_KEYBOARD_MAJOR_OPCODE),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        )
    };
    let state = |lock_group, group_lock, affect_mod_locks, mod_locks| {
        XWireRequest::XkbLatchLockState {
            affect_mod_locks,
            mod_locks,
            lock_group,
            group_lock,
            affect_mod_latches: 0,
            mod_latches: 0,
            latch_group: false,
            group_latch: 0,
        }
    };

    // A second group is out of range for a keymap that advertises one.
    let other_group = dispatch(state(true, 1, 0, 0));
    let [XClientOutput::Error(error)] = other_group.outputs.as_slice() else {
        panic!("naming a group this keymap has not got must be refused");
    };
    assert_eq!(error.code, XErrorCode::BadValue);
    assert_eq!(
        error.minor_code,
        u16::from(X_KEYBOARD_LATCH_LOCK_STATE_MINOR_OPCODE)
    );

    // Holding a modifier down is legal and this instance has no state for it.
    // BadImplementation says so; accepting and dropping it would leave a
    // client believing a modifier is held while every key arrives without it.
    let hold_control = dispatch(state(false, 0, 0x04, 0x04));
    let [XClientOutput::Error(error)] = hold_control.outputs.as_slice() else {
        panic!("a modifier lock this instance cannot hold must be refused");
    };
    assert_eq!(error.code, XErrorCode::BadImplementation);

    // Clearing what is already clear is satisfied, not refused.
    assert!(dispatch(state(true, 0, 0xff, 0)).outputs.is_empty());
}
