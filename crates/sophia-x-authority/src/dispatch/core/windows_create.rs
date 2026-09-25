// CreateWindow, ChangeWindowAttributes, GetWindowAttributes and the
// authority's own selection request: a window's attributes, from creation
// on. Included by dispatch.rs beside windows.rs; one module with it.

fn dispatch_window_creation_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    _properties: &mut XPropertyTable,
) -> XDispatchResult {
    match request {
                XWireRequest::Core(crate::XCoreRequest::CreateWindow {
                    packet,
                    event_mask,
                    parent,
                    background_pixmap,
                    background_pixel,
                    override_redirect,
                    depth,
                    visual,
                    colormap,
                    input_only,
                    copy_class_from_parent,
                    border_width,
                    win_gravity,
                    bit_gravity,
                    border_pixmap,
                    border_pixel,
                    ..
                }) => {
                    let kind = packet.kind.clone();
                    let namespace = packet.namespace;
                    let transaction = packet.transaction;
                    let XAuthorityRequestKind::CreateWindow { window, .. } = &kind else {
                        unreachable!("CreateWindow wire requests carry CreateWindow authority packets")
                    };
                    // VisibilityChange selected at creation: the window's
                    // occlusion is computed from now on.
                    if event_mask.is_some_and(|mask| mask & (1 << 16) != 0) {
                        runtime.note_visibility_interest(*window);
                    }
                    if runtime.resource_id_in_use(*window) {
                        return XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(crate::XClientError {
                                code: XErrorCode::BadIdChoice,
                                sequence: context.sequence,
                                resource_id: u32::try_from(window.local.raw()).unwrap_or(0),
                                minor_code: 0,
                                major_code: context.major_opcode,
                            })],
                            metadata_candidates: Vec::new(),
                        };
                    }
                    let (resolved_depth, resolved_visual, resolved_colormap) =
                        match resolve_window_visual(
                            runtime,
                            namespace,
                            parent,
                            depth,
                            visual,
                            colormap,
                        ) {
                            Ok(resolved) => resolved,
                            Err((code, resource_id)) => {
                                return XDispatchResult {
                                    response: None,
                                    outputs: vec![XClientOutput::Error(crate::XClientError {
                                        code,
                                        sequence: context.sequence,
                                        resource_id,
                                        minor_code: 0,
                                        major_code: context.major_opcode,
                                    })],
                                    metadata_candidates: Vec::new(),
                                };
                            }
                        };
                    // CopyFromParent under an InputOnly parent is InputOnly.
                    if let Some(refusal) = refused_window_attributes(
                        runtime,
                        namespace,
                        input_only || (copy_class_from_parent && runtime.window_is_input_only(parent)),
                        resolved_depth,
                        parent,
                        XRefusableAttributes {
                            background_pixmap,
                            background_pixel: background_pixel.is_some(),
                            border_pixmap,
                            border_pixel: border_pixel.is_some(),
                        },
                    ) {
                        return window_attribute_refusal(context, refusal);
                    }
                    // After the parent, as the reference orders its refusals:
                    // "The width and height must be nonzero, or a Value error
                    // results", and an InputOutput window cannot be created
                    // under an InputOnly parent (BadMatch); CopyFromParent
                    // under one is InputOnly.
                    let parent_input_only = runtime.window_is_input_only(parent);
                    if let XAuthorityRequestKind::CreateWindow { geometry, .. } = &kind
                        && (geometry.width <= 0 || geometry.height <= 0)
                    {
                        return XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(crate::XClientError {
                                code: XErrorCode::BadValue,
                                sequence: context.sequence,
                                resource_id: 0,
                                minor_code: 0,
                                major_code: context.major_opcode,
                            })],
                            metadata_candidates: Vec::new(),
                        };
                    }
                    if parent_input_only && !input_only && !copy_class_from_parent {
                        return XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(crate::XClientError {
                                code: XErrorCode::BadMatch,
                                sequence: context.sequence,
                                resource_id: u32::try_from(parent.local.raw()).unwrap_or(0),
                                minor_code: 0,
                                major_code: context.major_opcode,
                            })],
                            metadata_candidates: Vec::new(),
                        };
                    }
                    let input_only = input_only || (copy_class_from_parent && parent_input_only);
                    let mut response = runtime.apply(packet);
                    if response.outcome == XAuthorityResponseOutcome::Accepted
                        && let XAuthorityRequestKind::CreateWindow { window, .. } = &kind
                        && let Err(error) = runtime.set_window_parent(namespace, *window, parent) {
                            let _ = runtime.destroy_window(namespace, *window);
                            response = XAuthorityResponsePacket::rejected(transaction, error);
                        }
                    if response.outcome == XAuthorityResponseOutcome::Accepted
                        && let XAuthorityRequestKind::CreateWindow { window, .. } = &kind
                    {
                        if let Ok(surface) = runtime.set_window_override_redirect(
                            namespace,
                            *window,
                            override_redirect,
                        ) {
                            response.surfaces.clear();
                            response.surfaces.push(surface);
                        }
                        // Pixmap first, then pixel: the protocol orders the
                        // value list by bit and BackPixel is the later bit, so
                        // a request naming both means the pixel. A request
                        // naming neither leaves the background undefined,
                        // which is the default and is not black.
                        if let Some(background) = background_pixmap {
                            let _ = runtime.set_window_background(namespace, *window, background);
                        }
                        if let Some(pixel) = background_pixel {
                            let _ = runtime.set_window_background_pixel(namespace, *window, pixel);
                        }
                        runtime.set_window_visual(
                            *window,
                            resolved_depth,
                            resolved_visual,
                            resolved_colormap,
                        );
                        runtime.set_window_input_only(*window, input_only);
                        runtime.set_window_border_width(*window, border_width);
                        if let Some(gravity) = win_gravity {
                            runtime.set_window_gravity(*window, gravity);
                        }
                        if let Some(gravity) = bit_gravity {
                            runtime.set_window_bit_gravity(*window, gravity);
                        }
                    }
                    let mut outputs = outputs_from_authority_response(context, runtime, &kind, &response);
                    if response.outcome == XAuthorityResponseOutcome::Accepted
                        && let XAuthorityRequestKind::CreateWindow {
                            window, geometry, ..
                        } = kind
                    {
                        outputs.push(XClientOutput::Event(XClientEvent::CreateNotify {
                            sequence: context.sequence,
                            parent,
                            window,
                            x: clamp_i16(geometry.x),
                            y: clamp_i16(geometry.y),
                            width: clamp_u16(geometry.width),
                            height: clamp_u16(geometry.height),
                            border_width,
                            override_redirect,
                        }));
                    }
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Authority(mut packet) => {
                    if let XAuthorityRequestKind::RequestSelection {
                        target,
                        target_name,
                        ..
                    } = &mut packet.kind
                        && let Some(name) = atoms.name(*target)
                    {
                        *target_name = name.to_owned();
                    }
                    let kind = packet.kind.clone();
                    // The protocol's refusals for the selection requests,
                    // decided here where the atom table is: an atom that names
                    // nothing is BadAtom, a requestor that names no window is
                    // BadWindow (XTS Xlib5 XSetSelectionOwner 8, XConvertSelection 4-5).
                    let unknown_atom = |atom: crate::XAtom| atom != 0 && atoms.name(atom).is_none();
                    let refusal = match &kind {
                        XAuthorityRequestKind::SetSelectionOwner { selection, .. } if unknown_atom(*selection) => {
                            Some((XErrorCode::BadAtom, *selection))
                        }
                        XAuthorityRequestKind::RequestSelection { requestor, selection, target, property, .. } => {
                            // A requestor that names no window at all. One in
                            // another namespace keeps the confined answer, a
                            // failed conversion, which discloses nothing.
                            if matches!(
                                runtime.validate_window_access(packet.namespace, *requestor),
                                Err(XAuthorityRuntimeError::UnknownResource)
                            ) {
                                Some((XErrorCode::BadWindow, u32::try_from(requestor.local.raw()).unwrap_or(0)))
                            } else {
                                [*selection, *target, *property]
                                    .into_iter()
                                    .find(|atom| unknown_atom(*atom))
                                    .map(|atom| (XErrorCode::BadAtom, atom))
                            }
                        }
                        _ => None,
                    };
                    if let Some((code, resource_id)) = refusal {
                        return XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(crate::XClientError {
                                code,
                                sequence: context.sequence,
                                resource_id,
                                minor_code: 0,
                                major_code: context.major_opcode,
                            })],
                            metadata_candidates: Vec::new(),
                        };
                    }
                    // Whether the window was already mapped has to be read
                    // before the effect, because afterwards a redundant map
                    // and a real one look exactly alike.
                    let already_mapped =
                        if let XAuthorityRequestKind::MapWindow { window, .. } = kind {
                            runtime
                                .window_map_state(context.namespace, window)
                                .is_ok_and(|state| state != crate::XMapState::Unmapped)
                        } else {
                            false
                        };
                    let response = runtime.apply(packet);
                    if let XAuthorityRequestKind::RequestSelection { transfer, .. } = &kind {
                        runtime.set_pending_clipboard_byte_order(*transfer, context.byte_order);
                    }
                    if response.outcome == XAuthorityResponseOutcome::Accepted
                        && let XAuthorityRequestKind::SetSelectionOwner { selection, owner: Some(_), .. } = &kind
                    {
                        runtime.note_selection_requester(context.namespace, *selection, context.client_id);
                    }
                    let outputs = if let XAuthorityRequestKind::MapWindow { window, .. } = kind {
                        outputs_from_map_response(
                            context,
                            window,
                            already_mapped,
                            runtime.window_map_state(context.namespace, window).ok(),
                            runtime
                                .window_override_redirect(context.namespace, window)
                                .unwrap_or(false),
                            runtime.window_is_input_only(window),
                            runtime.window_visibility(context.namespace, window),
                            &response,
                        )
                    } else {
                        outputs_from_authority_response(context, runtime, &kind, &response)
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::ChangeWindowAttributes {
                    window,
                    event_mask,
                    background_pixmap,
                    background_pixel,
                    override_redirect,
                    cursor,
                    border_pixmap,
                    border_pixel,
                    win_gravity,
                    bit_gravity,
                    colormap,
                    ..
                }) => {
                    if event_mask.is_some_and(|mask| mask & (1 << 16) != 0) {
                        runtime.note_visibility_interest(window);
                    }
                    if runtime.validate_drawable_access(context.namespace, window).is_ok()
                        && let Ok((parent, _)) =
                            runtime.window_parent_and_children(context.namespace, window)
                        && let Some(refusal) = refused_window_attributes(
                            runtime,
                            context.namespace,
                            runtime.window_is_input_only(window),
                            runtime.window_visual(window).0,
                            parent,
                            XRefusableAttributes {
                                background_pixmap,
                                background_pixel: background_pixel.is_some(),
                                border_pixmap,
                                border_pixel: border_pixel.is_some(),
                            },
                        )
                    {
                        return window_attribute_refusal(context, refusal);
                    }
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
                    // The colormap names a second resource too, and a change
                    // is told to the window's ColormapChange selectors (t210).
                    let mut colormap_notice = None;
                    if let Some(raw) = colormap
                        && runtime
                            .validate_drawable_access(context.namespace, window)
                            .is_ok()
                    {
                        match runtime.change_window_colormap(context.namespace, window, raw) {
                            Ok(changed) => {
                                colormap_notice = changed.map(|colormap| {
                                    XClientOutput::Event(XClientEvent::ColormapNotify {
                                        sequence: context.sequence,
                                        window,
                                        colormap,
                                        new: true,
                                        state: colormap_state(colormap),
                                    })
                                });
                            }
                            Err(error) => {
                                let code = match error {
                                    crate::XWindowColormapError::Color => XErrorCode::BadColor,
                                    crate::XWindowColormapError::Match => XErrorCode::BadMatch,
                                };
                                return XDispatchResult {
                                    response: None,
                                    outputs: vec![XClientOutput::Error(crate::XClientError {
                                        code,
                                        sequence: context.sequence,
                                        resource_id: raw,
                                        minor_code: 0,
                                        major_code: context.major_opcode,
                                    })],
                                    metadata_candidates: Vec::new(),
                                };
                            }
                        }
                    }
                    // The cursor first, and separately: it names a second
                    // resource, so it is the one attribute here that can be
                    // refused for something other than the window. A refusal
                    // names the cursor, because that is what was wrong.
                    if let Some(cursor) = cursor
                        && runtime
                            .validate_drawable_access(context.namespace, window)
                            .is_ok()
                        && let Err(error) =
                            runtime.set_window_cursor(context.namespace, window, cursor)
                    {
                        let mut refusal = x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            cursor,
                        );
                        // A name that is no cursor is a Cursor error, which
                        // the generic mapping would call a Window.
                        if matches!(
                            error,
                            XAuthorityRuntimeError::UnknownResource
                                | XAuthorityRuntimeError::InvalidResource
                                | XAuthorityRuntimeError::WrongResourceKind
                        ) {
                            refusal.code = XErrorCode::BadCursor;
                        }
                        return XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(refusal)],
                            metadata_candidates: Vec::new(),
                        };
                    }
                    let outputs = if let Err(error) =
                        runtime.validate_drawable_access(context.namespace, window)
                    {
                        response = XAuthorityResponsePacket::rejected(transaction, error);
                        vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0)))]
                    } else {
                        // The background attributes take effect the next time
                        // the window is painted with its background, which is
                        // the next time it becomes viewable. Changing them on
                        // a window already on screen repaints nothing, which
                        // is what the protocol says: the background is used
                        // when the contents are lost, not when it is set.
                        if let Some(background) = background_pixmap {
                            let _ = runtime.set_window_background(
                                context.namespace,
                                window,
                                background,
                            );
                        }
                        if let Some(gravity) = win_gravity {
                            runtime.set_window_gravity(window, gravity);
                        }
                        if let Some(gravity) = bit_gravity {
                            runtime.set_window_bit_gravity(window, gravity);
                        }
                        if let Some(pixel) = background_pixel {
                            let _ =
                                runtime.set_window_background_pixel(context.namespace, window, pixel);
                        }
                        Vec::new()
                    };
                    let outputs = if !outputs.is_empty() {
                        outputs
                    } else if let Some(override_redirect) = override_redirect {
                        match runtime.set_window_override_redirect(
                            context.namespace,
                            window,
                            override_redirect,
                        ) {
                            Ok(surface) => {
                                response.surfaces.push(surface);
                                Vec::new()
                            }
                            Err(error) => {
                                response = XAuthorityResponsePacket::rejected(transaction, error);
                                vec![XClientOutput::Error(x_error_from_runtime(
                                    error,
                                    context.sequence,
                                    context.major_opcode,
                                    0,
                                    u32::try_from(window.local.raw()).unwrap_or(0)))]
                            }
                        }
                    } else {
                        Vec::new()
                    };
                    let mut outputs = outputs;
                    if outputs.is_empty() {
                        outputs.extend(colormap_notice);
                    }
                    XDispatchResult {
                        response: override_redirect.map(|_| response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::GetWindowAttributes { window }) => {
                    let output = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        XClientOutput::Reply(XClientReply::GetWindowAttributes {
                            sequence: context.sequence,
                            visual: X_SETUP_DEFAULT_VISUAL,
                            colormap: XResourceId::new(u64::from(X_SETUP_DEFAULT_COLORMAP), 1),
                            map_state: 2,
                            override_redirect: false,
                            bit_gravity: 0,
                            win_gravity: 1,
                        })
                    } else if let Err(error) = runtime.validate_window_access(context.namespace, window) {
                        XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(window.local.raw()).unwrap_or(0)))
                    } else {
                        let (_, visual, colormap) = runtime.window_visual(window);
                        let override_redirect = runtime
                            .window_override_redirect(context.namespace, window)
                            .unwrap_or(false);
                        let map_state = runtime
                            .window_map_state(context.namespace, window)
                            .map_or(0, x11_map_state);
                        XClientOutput::Reply(XClientReply::GetWindowAttributes {
                            sequence: context.sequence,
                            visual,
                            colormap,
                            map_state,
                            override_redirect,
                            bit_gravity: runtime.window_bit_gravity(window),
                            win_gravity: runtime.window_gravity(window),
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
        _ => unreachable!("request family checked before dispatch"),
    }
}
