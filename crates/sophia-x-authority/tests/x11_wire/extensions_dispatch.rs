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
        XWireRequest::Xi(sophia_x_authority::XInputRequest::XiGrabDevice {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            time: 0,
            cursor: None,
            device_id: 2,
            pointer_mode: 1,
            keyboard_mode: 1,
            owner_events: false,
            event_mask: vec![0x70],
        }),
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
        XWireRequest::Randr(sophia_x_authority::XRandrRequest::RandrQueryVersion {
            major_version: 1,
            minor_version: 5,
        })
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
        XWireRequest::Randr(sophia_x_authority::XRandrRequest::RandrSelectInput {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            enable: 0x000b,
        })
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
        XWireRequest::Randr(sophia_x_authority::XRandrRequest::RandrGetOutputPrimary {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        })
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
        XWireRequest::Randr(sophia_x_authority::XRandrRequest::RandrGetProviders {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        })
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
        XWireRequest::Randr(sophia_x_authority::XRandrRequest::RandrGetMonitors {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            get_active: true,
        })
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
        assert_eq!(request, XWireRequest::Randr(sophia_x_authority::XRandrRequest::RandrGetPanning { crtc }));

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
        XWireRequest::Randr(sophia_x_authority::XRandrRequest::RandrGetCrtcTransform {
            crtc: 0x1000_0001
        })
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
        XWireRequest::Randr(sophia_x_authority::XRandrRequest::RandrGetCrtcGamma {
            crtc: 0x1000_0001
        })
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
        XWireRequest::Randr(sophia_x_authority::XRandrRequest::RandrGetOutputProperty {
            output: 0x2000_0001,
            property,
            long_length: 128,
            ..
        }) if property == edid
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
                XWireRequest::Xfixes(sophia_x_authority::XFixesRequest::XfixesSetRegion {
                    rectangles: ref decoded,
                    ..
                }) if decoded == &rectangles
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
    let destroy = XWireRequest::Xfixes(sophia_x_authority::XFixesRequest::XfixesDestroyRegion { region: region_id });
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
        XWireRequest::Xi(sophia_x_authority::XInputRequest::XiChangeCursor {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            cursor: None,
        }),
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
        XWireRequest::Xfixes(sophia_x_authority::XFixesRequest::XfixesSelectSelectionInput {
            selection: X_ATOM_PRIMARY,
            event_mask: 0b111,
            ..
        })
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
