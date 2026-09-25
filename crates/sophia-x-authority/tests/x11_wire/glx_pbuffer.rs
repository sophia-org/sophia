// GLX offscreen drawables: the pbuffer a GL client creates before it has a window.

/// Builds a `CreatePbuffer` request. `num_attribs` counts pairs, not words.
fn glx_create_pbuffer_request(
    byte_order: XByteOrder,
    fbconfig: u32,
    pbuffer: u32,
    attributes: &[(u32, u32)],
) -> Vec<u8> {
    let mut out = vec![X_GLX_MAJOR_OPCODE, X_GLX_CREATE_PBUFFER_MINOR_OPCODE];
    let words = 5 + attributes.len() * 2;
    push_u16(&mut out, byte_order, u16::try_from(words).unwrap());
    push_u32(&mut out, byte_order, 0);
    push_u32(&mut out, byte_order, fbconfig);
    push_u32(&mut out, byte_order, pbuffer);
    push_u32(&mut out, byte_order, u32::try_from(attributes.len()).unwrap());
    for (name, value) in attributes {
        push_u32(&mut out, byte_order, *name);
        push_u32(&mut out, byte_order, *value);
    }
    out
}

fn glx_destroy_pbuffer_request(byte_order: XByteOrder, pbuffer: u32) -> Vec<u8> {
    let mut out = vec![X_GLX_MAJOR_OPCODE, X_GLX_DESTROY_PBUFFER_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, pbuffer);
    out
}

/// The exact request Helium's GL layer sends to bootstrap a display.
#[test]
fn glx_decoder_accepts_the_initialization_pbuffer_in_both_byte_orders() {
    let namespace = NamespaceId::from_raw(71);
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let request = glx_create_pbuffer_request(
            byte_order,
            1,
            0x220301,
            &[
                (X_GLX_PBUFFER_WIDTH_ATTRIBUTE, 1),
                (X_GLX_PBUFFER_HEIGHT_ATTRIBUTE, 1),
            ],
        );
        assert_eq!(
            decode_x11_core_request(context(namespace, 1, byte_order), &request).unwrap(),
            XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxCreatePbuffer {
                screen: 0,
                fbconfig: 1,
                pbuffer: XResourceId::new(0x220301, 1),
                width: 1,
                height: 1,
                largest: false,
            })
        );
    }
}

/// Height is the lower attribute number, so a swap decodes as a transposed
/// surface rather than failing.
#[test]
fn glx_pbuffer_attributes_are_read_by_name_not_position() {
    let namespace = NamespaceId::from_raw(72);
    let request = glx_create_pbuffer_request(
        XByteOrder::LittleEndian,
        2,
        0x220302,
        &[
            (X_GLX_PBUFFER_HEIGHT_ATTRIBUTE, 48),
            (X_GLX_PBUFFER_WIDTH_ATTRIBUTE, 64),
        ],
    );
    assert_eq!(
        decode_x11_core_request(context(namespace, 1, XByteOrder::LittleEndian), &request).unwrap(),
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxCreatePbuffer {
            screen: 0,
            fbconfig: 2,
            pbuffer: XResourceId::new(0x220302, 1),
            width: 64,
            height: 48,
            largest: false,
        })
    );
}

/// A client may ask for more than Sophia implements; only the list's length is
/// its business.
#[test]
fn glx_pbuffer_ignores_attributes_it_does_not_implement() {
    let namespace = NamespaceId::from_raw(73);
    let request = glx_create_pbuffer_request(
        XByteOrder::LittleEndian,
        1,
        0x220303,
        &[
            (0x801B, 1),
            (X_GLX_PBUFFER_WIDTH_ATTRIBUTE, 16),
            (0x9999, 7),
            (X_GLX_PBUFFER_HEIGHT_ATTRIBUTE, 16),
            (X_GLX_LARGEST_PBUFFER_ATTRIBUTE, 1),
        ],
    );
    assert_eq!(
        decode_x11_core_request(context(namespace, 1, XByteOrder::LittleEndian), &request).unwrap(),
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxCreatePbuffer {
            screen: 0,
            fbconfig: 1,
            pbuffer: XResourceId::new(0x220303, 1),
            width: 16,
            height: 16,
            largest: true,
        })
    );

    // A pair count that disagrees with the bytes that arrived is malformed. The
    // request's own length stays honest, so this is the arm's check firing and
    // not the outer one.
    let mut mismatched = request.clone();
    let claimed = u32::try_from(request[20..].len() / 8 + 1).unwrap();
    mismatched[16..20].copy_from_slice(&claimed.to_le_bytes());
    assert!(matches!(
        decode_x11_core_request(context(namespace, 1, XByteOrder::LittleEndian), &mismatched),
        Err(XWireParseError::InvalidLength { .. })
    ));
}

/// The differential test: a created pbuffer answers core `GetGeometry`.
///
/// A GL client names the same id for the drawable it just created, and that
/// request is what decides whether the drawable exists at all. Before pbuffers
/// existed it returned `BadWindow`, which is the error a physical session failed
/// on six times.
#[test]
fn a_created_pbuffer_answers_core_get_geometry() {
    let namespace = NamespaceId::from_raw(74);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let pbuffer = 0x220304;

    let create = decode_x11_core_request(
        context(namespace, 1, XByteOrder::LittleEndian),
        &glx_create_pbuffer_request(
            XByteOrder::LittleEndian,
            2,
            pbuffer,
            &[
                (X_GLX_PBUFFER_WIDTH_ATTRIBUTE, 64),
                (X_GLX_PBUFFER_HEIGHT_ATTRIBUTE, 48),
            ],
        ),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(result.outputs.is_empty(), "creating a pbuffer must not error");

    let geometry = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 14),
        XWireRequest::Core(sophia_x_authority::XCoreRequest::GetGeometry {
            drawable: XResourceId::new(u64::from(pbuffer), 1),
        }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        geometry.outputs.as_slice(),
        [XClientOutput::Reply(XClientReply::GetGeometry {
            depth: 32,
            geometry: Rect { width: 64, height: 48, .. },
            border_width: 0,
            ..
        })],
    ));

    // The fix must not degenerate into answering for anything: an id that was
    // never created still reports the error it always did, naming itself.
    let unknown = 0x220999;
    let missing = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 14),
        XWireRequest::Core(sophia_x_authority::XCoreRequest::GetGeometry {
            drawable: XResourceId::new(u64::from(unknown), 1),
        }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    // BadDrawable, not BadWindow: GetGeometry takes a drawable, and an id that
    // names nothing is not a window of the wrong kind.
    assert!(matches!(
        missing.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadDrawable,
            resource_id,
            ..
        })] if *resource_id == unknown
    ));

    // Destroying it takes the answer away again.
    let destroy = decode_x11_core_request(
        context(namespace, 5, XByteOrder::LittleEndian),
        &glx_destroy_pbuffer_request(XByteOrder::LittleEndian, pbuffer),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 6, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        destroy,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(result.outputs.is_empty());
    let after = dispatch_x11_wire_request(
        dispatch_context(namespace, 7, XByteOrder::LittleEndian, 14),
        XWireRequest::Core(sophia_x_authority::XCoreRequest::GetGeometry {
            drawable: XResourceId::new(u64::from(pbuffer), 1),
        }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    // Destroyed, so the id names nothing: BadDrawable for the same reason.
    assert!(matches!(
        after.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadDrawable,
            ..
        })]
    ));
}

/// The advertised maximum and the refusal threshold are one number.
#[test]
fn a_pbuffer_beyond_the_advertised_maximum_is_refused_or_clamped() {
    let namespace = NamespaceId::from_raw(75);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    // Read the bound out of the encoded catalog, so editing one without the
    // other fails here.
    let configs = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxGetFbConfigs { screen: 0 }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian)
    .remove(0);
    let attributes = read_u32(XByteOrder::LittleEndian, &configs[12..16]) as usize;
    let advertised = (0..attributes)
        .map(|index| {
            let at = 32 + index * 8;
            (
                read_u32(XByteOrder::LittleEndian, &configs[at..at + 4]),
                read_u32(XByteOrder::LittleEndian, &configs[at + 4..at + 8]),
            )
        })
        .find(|(name, _)| *name == 0x8016)
        .expect("the catalog advertises a maximum pbuffer width")
        .1;
    assert!(
        advertised > 0,
        "a drawable type we implement needs a stated bound"
    );

    let oversized = |largest: bool, id: u32| {
        decode_x11_core_request(
            context(namespace, 2, XByteOrder::LittleEndian),
            &glx_create_pbuffer_request(
                XByteOrder::LittleEndian,
                1,
                id,
                &[
                    (X_GLX_PBUFFER_WIDTH_ATTRIBUTE, advertised + 1),
                    (X_GLX_PBUFFER_HEIGHT_ATTRIBUTE, 1),
                    (X_GLX_LARGEST_PBUFFER_ATTRIBUTE, u32::from(largest)),
                ],
            ),
        )
        .unwrap()
    };

    let refused = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        oversized(false, 0x220305),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        refused.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadAlloc,
            ..
        })]
    ));

    // Asking for the largest available clamps rather than failing.
    let clamped = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        oversized(true, 0x220306),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(clamped.outputs.is_empty());
    let geometry = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 14),
        XWireRequest::Core(sophia_x_authority::XCoreRequest::GetGeometry {
            drawable: XResourceId::new(0x220306, 1),
        }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        geometry.outputs.as_slice(),
        [XClientOutput::Reply(XClientReply::GetGeometry {
            geometry: Rect { width, .. },
            ..
        })] if u32::try_from(*width).unwrap() == advertised
    ));
}

/// The GLX 1.3 requests a client may reach after the drawable API it is offered.
///
/// A server claiming a version owes the requests that version introduced. These two
/// are the remainder of the 1.3 surface Sophia advertises, once the pbuffer half is
/// implemented and GLX pixmaps are withdrawn rather than promised.
#[test]
fn glx_answers_the_remaining_thirteen_requests() {
    let namespace = NamespaceId::from_raw(79);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let glx_context = 0x220401;
    let pbuffer = 0x220402;

    // A context to ask about.
    let mut create = vec![X_GLX_MAJOR_OPCODE, 3u8];
    push_u16(&mut create, XByteOrder::LittleEndian, 6);
    push_u32(&mut create, XByteOrder::LittleEndian, glx_context);
    push_u32(&mut create, XByteOrder::LittleEndian, X_SETUP_DEFAULT_VISUAL);
    push_u32(&mut create, XByteOrder::LittleEndian, 0);
    push_u32(&mut create, XByteOrder::LittleEndian, 0);
    create.extend_from_slice(&[1, 0, 0, 0]);
    let create = decode_x11_core_request(
        context(namespace, 1, XByteOrder::LittleEndian),
        &create,
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    // QueryContext answers with the configuration the context was made from.
    let mut query = vec![X_GLX_MAJOR_OPCODE, X_GLX_QUERY_CONTEXT_MINOR_OPCODE];
    push_u16(&mut query, XByteOrder::LittleEndian, 2);
    push_u32(&mut query, XByteOrder::LittleEndian, glx_context);
    let query = decode_x11_core_request(
        context(namespace, 3, XByteOrder::LittleEndian),
        &query,
    )
    .unwrap();
    assert_eq!(
        query,
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxQueryContext {
            context: XResourceId::new(u64::from(glx_context), 1),
        })
    );
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        query,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        result.outputs.as_slice(),
        [XClientOutput::Reply(XClientReply::GlxDrawableAttributes { .. })]
    ));

    // An unknown context is still refused.
    let mut unknown = vec![X_GLX_MAJOR_OPCODE, X_GLX_QUERY_CONTEXT_MINOR_OPCODE];
    push_u16(&mut unknown, XByteOrder::LittleEndian, 2);
    push_u32(&mut unknown, XByteOrder::LittleEndian, 0x220999);
    let unknown = decode_x11_core_request(
        context(namespace, 5, XByteOrder::LittleEndian),
        &unknown,
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 6, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        unknown,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        result.outputs.as_slice(),
        [XClientOutput::Error(_)]
    ));

    // ChangeDrawableAttributes validates its drawable and records nothing.
    let created = decode_x11_core_request(
        context(namespace, 7, XByteOrder::LittleEndian),
        &glx_create_pbuffer_request(
            XByteOrder::LittleEndian,
            1,
            pbuffer,
            &[
                (X_GLX_PBUFFER_WIDTH_ATTRIBUTE, 8),
                (X_GLX_PBUFFER_HEIGHT_ATTRIBUTE, 8),
            ],
        ),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 8, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        created,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let mut change = vec![
        X_GLX_MAJOR_OPCODE,
        X_GLX_CHANGE_DRAWABLE_ATTRIBUTES_MINOR_OPCODE,
    ];
    push_u16(&mut change, XByteOrder::LittleEndian, 5);
    push_u32(&mut change, XByteOrder::LittleEndian, pbuffer);
    push_u32(&mut change, XByteOrder::LittleEndian, 1);
    push_u32(&mut change, XByteOrder::LittleEndian, 0x801D);
    push_u32(&mut change, XByteOrder::LittleEndian, 0);
    let change = decode_x11_core_request(
        context(namespace, 9, XByteOrder::LittleEndian),
        &change,
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 10, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        change,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        result.outputs.is_empty(),
        "setting an event mask Sophia never sends is not an error"
    );
}

/// The drawable types Sophia advertises are the ones it implements.
///
/// The catalog used to promise pixmap drawables with none of the four requests
/// that make one behind it -- the same advertise-then-refuse that cost this tree
/// a physical run twice. Withdrawing the bit is what makes the promise true, and
/// this keeps the two from parting again.
#[test]
fn glx_advertises_only_the_drawable_types_it_implements() {
    let namespace = NamespaceId::from_raw(77);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let configs = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxGetFbConfigs { screen: 0 }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian)
    .remove(0);
    let attributes = read_u32(XByteOrder::LittleEndian, &configs[12..16]) as usize;
    let drawable_type = (0..attributes)
        .map(|index| {
            let at = 32 + index * 8;
            (
                read_u32(XByteOrder::LittleEndian, &configs[at..at + 4]),
                read_u32(XByteOrder::LittleEndian, &configs[at + 4..at + 8]),
            )
        })
        .find(|(name, _)| *name == 0x8010)
        .expect("the catalog states which drawable types it supports")
        .1;

    // Window and pbuffer always. Pixmap only where a provider backs it, and no
    // provider is configured here.
    assert_eq!(drawable_type & 0x1, 0x1, "window drawables are implemented");
    assert_eq!(drawable_type & 0x4, 0x4, "pbuffer drawables are implemented");
    assert_eq!(
        drawable_type & 0x2,
        0,
        "GLX pixmaps are unbacked here, so they must not be advertised"
    );

    // And the constructors behind the withdrawn bit refuse, so the bit cannot
    // be honoured without the capability that advertises it.
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    assert!(!runtime.pixmap_textures_supported());
    let refusal = dispatch_x11_wire_request(
        dispatch_context(namespace, 7, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxCreatePixmap {
            screen: 0,
            fbconfig: 2,
            pixmap: XResourceId::new(0x420_001, 1),
            glx_pixmap: XResourceId::new(0x420_002, 1),
            target: None,
            format: None,
            mipmap: None,
        }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        matches!(
            refusal.outputs.as_slice(),
            [XClientOutput::Error(error)] if error.code == XErrorCode::GlxBadFbConfig
        ),
        "an unbacked GLX pixmap must be refused, answered {:?}",
        refusal.outputs,
    );
}

/// A GL client imports its own buffers against the pbuffer it just created.
///
/// This is the pair statement with the guard below: DRI3 pixels are
/// client-allocated, so a drawable with no server storage is a legal target here
/// and is not one for core drawing. A physical run failed on exactly this --
/// `dri3_get_pixmap_buffer` naming the pbuffer and taking `BadWindow`.
#[test]
fn dri3_admits_a_pbuffer_as_a_client_allocated_target() {
    let namespace = NamespaceId::from_raw(78);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let pbuffer = 0x220308;

    let create = decode_x11_core_request(
        context(namespace, 1, XByteOrder::LittleEndian),
        &glx_create_pbuffer_request(
            XByteOrder::LittleEndian,
            1,
            pbuffer,
            &[
                (X_GLX_PBUFFER_WIDTH_ATTRIBUTE, 64),
                (X_GLX_PBUFFER_HEIGHT_ATTRIBUTE, 64),
            ],
        ),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    // The request Mesa names in its error, and the one it sends first.
    assert!(
        runtime
            .validate_dri3_drawable_access(namespace, XResourceId::new(u64::from(pbuffer), 1))
            .is_ok(),
        "a client-allocated import may name an offscreen drawable"
    );

    // GetSupportedModifiers takes a drawable despite the spec calling it a window.
    let modifiers = decode_x11_core_request(
        context(namespace, 3, XByteOrder::LittleEndian),
        &dri3_get_supported_modifiers_request(XByteOrder::LittleEndian, pbuffer, 24, 32),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, X_DRI3_MAJOR_OPCODE),
        modifiers,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        matches!(
            result.outputs.as_slice(),
            [XClientOutput::Reply(
                XClientReply::Dri3GetSupportedModifiers { .. }
            )]
        ),
        "supported modifiers must answer for an offscreen drawable"
    );

    // A destroyed pbuffer stops being a legal target, and an id that never
    // existed was never one.
    let destroy = decode_x11_core_request(
        context(namespace, 5, XByteOrder::LittleEndian),
        &glx_destroy_pbuffer_request(XByteOrder::LittleEndian, pbuffer),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 6, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        destroy,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        runtime
            .validate_dri3_drawable_access(namespace, XResourceId::new(u64::from(pbuffer), 1))
            .is_err()
    );
    assert!(
        runtime
            .validate_dri3_drawable_access(namespace, XResourceId::new(0x220999, 1))
            .is_err()
    );
}

/// A pbuffer has no storage, so core drawing must keep refusing it. This pins
/// the deliberate narrowness of `validate_drawable_access`.
#[test]
fn core_drawing_still_refuses_an_offscreen_drawable() {
    let namespace = NamespaceId::from_raw(76);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let pbuffer = 0x220307;

    let create = decode_x11_core_request(
        context(namespace, 1, XByteOrder::LittleEndian),
        &glx_create_pbuffer_request(
            XByteOrder::LittleEndian,
            1,
            pbuffer,
            &[
                (X_GLX_PBUFFER_WIDTH_ATTRIBUTE, 8),
                (X_GLX_PBUFFER_HEIGHT_ATTRIBUTE, 8),
            ],
        ),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(
        runtime
            .validate_drawable_access(namespace, XResourceId::new(u64::from(pbuffer), 1))
            .is_err(),
        "a drawable with no pixels is not a drawing target"
    );
}

#[test]
fn glx_advertises_the_es_profiles_a_translating_client_needs() {
    // A client that reaches a GL driver by translating to OpenGL ES -- which is
    // how Chromium's ANGLE works -- asks for an ES-profile context, and libGL
    // refuses that request client-side against a server which does not
    // advertise the profile, before the server ever sees it. A client rendering
    // desktop GL never notices, which is why one browser rendered here and
    // another waited forever.
    let namespace = NamespaceId::from_raw(66);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let request = decode_x11_core_request(
        context(namespace, 41, XByteOrder::LittleEndian),
        &{
            let mut out = vec![X_GLX_MAJOR_OPCODE, X_GLX_QUERY_SERVER_STRING_MINOR_OPCODE];
            push_u16(&mut out, XByteOrder::LittleEndian, 3);
            push_u32(&mut out, XByteOrder::LittleEndian, 0);
            // 3 selects the extension string.
            push_u32(&mut out, XByteOrder::LittleEndian, 3);
            out
        },
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let XClientOutput::Reply(XClientReply::GlxString { value, .. }) = &result.outputs[0] else {
        panic!("expected a GLX string reply, got {:?}", result.outputs[0]);
    };
    for profile in [
        "GLX_EXT_create_context_es_profile",
        "GLX_EXT_create_context_es2_profile",
    ] {
        assert!(
            value.split(' ').any(|name| name == profile),
            "{profile} must be advertised; got {value:?}",
        );
    }
}

fn glx_fb_config_reply(pixmap_textures: bool) -> Vec<Vec<(u32, u32)>> {
    let namespace = NamespaceId::from_raw(91);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    runtime.set_pixmap_textures_supported(pixmap_textures);
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxGetFbConfigs { screen: 0 }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    match result.outputs.as_slice() {
        [XClientOutput::Reply(XClientReply::GlxFbConfigs { configs, .. })] => configs.clone(),
        other => panic!("GetFBConfigs answered {other:?}"),
    }
}

fn attribute(config: &[(u32, u32)], attribute: u32) -> Option<u32> {
    config
        .iter()
        .find(|(name, _)| *name == attribute)
        .map(|(_, value)| *value)
}

/// Without a provider that backs them, the catalog is exactly what it was.
#[test]
fn pixmap_texture_capability_off_answers_the_original_three_rows() {
    let configs = glx_fb_config_reply(false);
    assert_eq!(configs.len(), X_GLX_BASE_FB_CONFIG_COUNT);
    for (index, config) in configs.iter().enumerate() {
        let id = u32::try_from(index).unwrap() + 1;
        assert_eq!(attribute(config, X_GLX_FBCONFIG_ID_ATTRIBUTE), Some(id));
        assert_eq!(
            attribute(config, X_GLX_DRAWABLE_TYPE_ATTRIBUTE),
            Some(X_GLX_DRAWABLE_TYPE_MASK),
            "config {id} must not claim pixmap drawables",
        );
        assert_eq!(attribute(config, X_GLX_STENCIL_SIZE_ATTRIBUTE), Some(0));
        assert_eq!(
            attribute(config, X_GLX_BIND_TO_TEXTURE_RGBA_ATTRIBUTE),
            None,
            "config {id} must not advertise bind-to-texture",
        );
    }
}

/// With one, the original rows keep their identifiers and gain the pixmap bit,
/// and the stencil rows a deriving client needs appear behind them.
#[test]
fn pixmap_texture_capability_on_adds_stencil_rows_and_bind_attributes() {
    let configs = glx_fb_config_reply(true);
    assert_eq!(configs.len(), X_GLX_FB_CONFIGS.len());
    for (index, config) in configs.iter().enumerate() {
        let id = u32::try_from(index).unwrap() + 1;
        assert_eq!(attribute(config, X_GLX_FBCONFIG_ID_ATTRIBUTE), Some(id));
        assert_eq!(
            attribute(config, X_GLX_DRAWABLE_TYPE_ATTRIBUTE),
            Some(X_GLX_DRAWABLE_TYPE_MASK_WITH_PIXMAPS),
        );
        assert_eq!(
            attribute(config, X_GLX_BIND_TO_TEXTURE_TARGETS_ATTRIBUTE),
            Some(X_GLX_TEXTURE_TARGETS_ALL),
        );
    }
    // The whole point of the stencil rows: a client asking for RGBA8 with
    // depth 24 and stencil 8 must find one, or it refuses to initialise.
    let rgba_depth_stencil = configs.iter().filter(|config| {
        attribute(config, X_GLX_ALPHA_SIZE_ATTRIBUTE) == Some(8)
            && attribute(config, X_GLX_DEPTH_SIZE_ATTRIBUTE) == Some(24)
            && attribute(config, X_GLX_STENCIL_SIZE_ATTRIBUTE) == Some(8)
            && attribute(config, X_GLX_BIND_TO_TEXTURE_RGBA_ATTRIBUTE) == Some(1)
            && attribute(config, X_GLX_DOUBLEBUFFER_ATTRIBUTE) == Some(1)
    });
    assert!(
        rgba_depth_stencil.count() > 0,
        "no row answers RGBA8 + depth24 + stencil8 + bind-to-texture",
    );
}
