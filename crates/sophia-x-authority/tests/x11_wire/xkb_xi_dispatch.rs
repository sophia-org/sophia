// XKB, XI2, MIT-SHM segments, XF86VidMode and XC-MISC through dispatch.
// Included from x11_wire.rs beside extensions_dispatch.rs (t026).

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
