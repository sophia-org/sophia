fn dispatch_core_window_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::Core(crate::XCoreRequest::CreateWindow { .. })
            | XWireRequest::Authority(..)
            | XWireRequest::Core(crate::XCoreRequest::ChangeWindowAttributes { .. })
            | XWireRequest::Core(crate::XCoreRequest::GetWindowAttributes { .. })
            | XWireRequest::Core(crate::XCoreRequest::DestroyWindow { .. })
            | XWireRequest::Core(crate::XCoreRequest::ReparentWindow { .. })
            | XWireRequest::Core(crate::XCoreRequest::DestroySubwindows { .. })
            | XWireRequest::Core(crate::XCoreRequest::MapSubwindows { .. })
            | XWireRequest::Core(crate::XCoreRequest::UnmapSubwindows { .. })
            | XWireRequest::Core(crate::XCoreRequest::CirculateWindow { .. })
            | XWireRequest::Core(crate::XCoreRequest::UnmapWindow { .. })
            | XWireRequest::Core(crate::XCoreRequest::ConfigureWindow { .. })
            | XWireRequest::Core(crate::XCoreRequest::GetGeometry { .. })
            | XWireRequest::Core(crate::XCoreRequest::GetImage { .. })
            | XWireRequest::Core(crate::XCoreRequest::QueryTree { .. })
    ) {
        return Unhandled(request);
    }
    let mut result = match request {
                request @ (XWireRequest::Core(crate::XCoreRequest::CreateWindow { .. }) | XWireRequest::Authority(..) | XWireRequest::Core(crate::XCoreRequest::ChangeWindowAttributes { .. }) | XWireRequest::Core(crate::XCoreRequest::GetWindowAttributes { .. })) => dispatch_window_creation_request(context, request, runtime, atoms, properties),
                request @ (XWireRequest::Core(crate::XCoreRequest::DestroyWindow { .. }) | XWireRequest::Core(crate::XCoreRequest::ReparentWindow { .. }) | XWireRequest::Core(crate::XCoreRequest::DestroySubwindows { .. }) | XWireRequest::Core(crate::XCoreRequest::MapSubwindows { .. }) | XWireRequest::Core(crate::XCoreRequest::UnmapSubwindows { .. }) | XWireRequest::Core(crate::XCoreRequest::CirculateWindow { .. }) | XWireRequest::Core(crate::XCoreRequest::UnmapWindow { .. })) => dispatch_window_hierarchy_request(context, request, runtime, atoms, properties),
                XWireRequest::Core(crate::XCoreRequest::ConfigureWindow {
                    window,
                    x,
                    y,
                    width,
                    height,
                    border_width,
                    sibling,
                    stack_mode,
                    ..
                }) => {
                    // The root is configured by nobody: a request on it has
                    // no effect and no error. Then the window, then its
                    // values, in the reference's order: an id that names no
                    // window is BadWindow before a zero size is BadValue.
                    if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        return Handled(XDispatchResult {
                            response: None,
                            outputs: Vec::new(),
                            metadata_candidates: Vec::new(),
                        });
                    }
                    if let Err(error) = runtime.validate_window_access(context.namespace, window) {
                        return Handled(XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0),
                            ))],
                            metadata_candidates: Vec::new(),
                        });
                    }
                    if width == Some(0) || height == Some(0) {
                        return Handled(XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(crate::XClientError {
                                code: XErrorCode::BadValue,
                                sequence: context.sequence,
                                resource_id: 0,
                                minor_code: 0,
                                major_code: context.major_opcode,
                            })],
                            metadata_candidates: Vec::new(),
                        });
                    }
                    // A sibling is only meaningful with a stack-mode, and must
                    // be a sibling: "If a sibling is specified without a
                    // stack-mode or the window is not actually a sibling, a
                    // Match error results." An id that names no window at all
                    // is a Window error, which the restack below reports.
                    if let Some(sibling) = sibling
                        && runtime.validate_window_access(context.namespace, window).is_ok()
                        && (stack_mode.is_none()
                            || runtime
                                .window_parent_and_children(context.namespace, sibling)
                                .ok()
                                .zip(runtime.window_parent_and_children(context.namespace, window).ok())
                                .is_some_and(|((sibling_parent, _), (parent, _))| sibling_parent != parent))
                    {
                        return Handled(XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(crate::XClientError {
                                code: XErrorCode::BadMatch,
                                sequence: context.sequence,
                                resource_id: u32::try_from(sibling.local.raw()).unwrap_or(0),
                                minor_code: 0,
                                major_code: context.major_opcode,
                            })],
                            metadata_candidates: Vec::new(),
                        });
                    }
                    // An InputOnly window has no border to be wide (t216).
                    if border_width.is_some_and(|width| width != 0)
                        && runtime.window_is_input_only(window)
                    {
                        return Handled(window_attribute_refusal(context, (XErrorCode::BadMatch, 0)));
                    }
                    let before = runtime.window_geometry(context.namespace, window).ok();
                    // The border width is a stored fact: changed here when
                    // asked, reported below with the rest.
                    let border_before = runtime.window_border_width(window);
                    let border_changed = border_width.is_some_and(|asked| asked != border_before)
                        && runtime.validate_window_access(context.namespace, window).is_ok();
                    // The siblings' order before, so a restack that changed
                    // nothing reports nothing and one that did names the
                    // sibling now beneath the window.
                    let siblings_of = |runtime: &XAuthorityRuntime| {
                        runtime
                            .window_parent_and_children(context.namespace, window)
                            .ok()
                            .map(|(parent, _)| {
                                runtime
                                    .window_parent_and_children(context.namespace, parent)
                                    .map(|(_, children)| children)
                                    .unwrap_or_default()
                            })
                            .unwrap_or_default()
                    };
                    let siblings_before = siblings_of(runtime);
                    let mut gravity_surfaces = Vec::new();
                    let mut bit_gravity_packet: Option<XAuthorityResponsePacket> = None;
                    let client_controls = runtime
                        .client_controls_window_geometry(context.namespace, window)
                        .unwrap_or(false);
                    let configure = runtime
                        .client_controls_window_geometry(context.namespace, window)
                        .and_then(|client_controls| {
                            if client_controls {
                                runtime.configure_window_geometry(
                                    context.namespace,
                                    window,
                                    XWindowGeometryUpdate {
                                        x,
                                        y,
                                        width,
                                        height,
                                        generation: u64::from(context.sequence),
                                    },
                                )
                            } else {
                                Ok(())
                            }
                        });
                    let mut restacked = None;
                    let configure = configure.and_then(|()| {
                        if client_controls && (sibling.is_some() || stack_mode.is_some()) {
                            restacked = Some(runtime.restack_window(
                                context.namespace,
                                window,
                                sibling,
                                stack_mode,
                            )?);
                        }
                        Ok(())
                    });
                    let border_changed = border_changed && configure.is_ok();
                    if border_changed {
                        runtime.set_window_border_width(window, border_width.unwrap_or(0));
                    }
                    let outputs = if let Err(error) = configure {
                        vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(window.local.raw()).unwrap_or(0)))]
                    } else {
                        // A stacking change alone is a configuration change
                        // too: "Raise lowest window to top" owes the same
                        // ConfigureNotify a move does, with above-sibling
                        // naming the sibling now beneath (XTS ConfigureNotify
                        // 1-2). Nothing changed, nothing reported.
                        let siblings_after = siblings_of(runtime);
                        let restacked_order = restacked.is_some() && siblings_after != siblings_before;
                        let above_sibling = siblings_after
                            .iter()
                            .position(|sibling| *sibling == window)
                            .and_then(|index| index.checked_sub(1))
                            .map(|index| siblings_after[index]);
                        match runtime.window_geometry(context.namespace, window) {
                            Ok(geometry) if before != Some(geometry) || restacked_order || border_changed || !client_controls => {
                                let override_redirect = runtime
                                    .window_override_redirect(context.namespace, window)
                                    .unwrap_or(false);
                                let mut outputs = vec![XClientOutput::Event(XClientEvent::ConfigureNotify {
                                    sequence: context.sequence,
                                    synthetic: !client_controls,
                                    event: window,
                                    window,
                                    above_sibling,
                                    x: clamp_i16(geometry.x),
                                    y: clamp_i16(geometry.y),
                                    width: clamp_u16(geometry.width),
                                    height: clamp_u16(geometry.height),
                                    border_width: runtime.window_border_width(window),
                                    override_redirect,
                                })];
                                // A resize moves the children by their
                                // win-gravity after the ConfigureNotify, as
                                // dix's ResizeChildrenWinSize does (t199).
                                if client_controls && let Some(before) = before {
                                    gravity_outputs(context, runtime, window, before, geometry, &mut outputs, &mut gravity_surfaces);
                                    bit_gravity_packet =
                                        contents_outputs(context, runtime, window, before, geometry, &mut outputs);
                                }
                                outputs
                            }
                            Ok(_) => Vec::new(),
                            Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0)))],
                        }
                    };
                    let border_packet = border_changed.then(|| {
                        runtime.republish_border_geometry(context.transaction, context.namespace, window)
                    }).flatten();
                    // A child unmapped by its gravity is a surface change too.
                    let response = if restacked.is_some()
                        || !gravity_surfaces.is_empty()
                        || bit_gravity_packet.is_some()
                        || border_packet.is_some()
                    {
                        let mut response = XAuthorityResponsePacket::accepted(context.transaction);
                        response.surfaces.extend(restacked);
                        response.surfaces.extend(gravity_surfaces);
                        if let Some(presented) = bit_gravity_packet {
                            response.surfaces.extend(presented.surfaces);
                            response.transactions.extend(presented.transactions);
                        }
                        if let Some(presented) = border_packet {
                            response.surfaces.extend(presented.surfaces);
                            response.transactions.extend(presented.transactions);
                        }
                        Some(response)
                    } else {
                        None
                    };
                    XDispatchResult {
                        response,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::GetGeometry { drawable }) => {
                    // Every kind of drawable answers the same four facts, so the
                    // resolver states them once rather than each kind being tried
                    // in turn here.
                    //
                    // A miss answers BadDrawable, not BadWindow. This request
                    // takes a DRAWABLE, and a pixmap id that names nothing is
                    // not a bad window: reporting the window error tells a
                    // client its pixmap was the wrong kind of thing rather than
                    // that it does not exist.
                    let output = match runtime.drawable_facts(context.namespace, drawable) {
                        Ok(facts) => XClientOutput::Reply(XClientReply::GetGeometry {
                            sequence: context.sequence,
                            // An InputOnly window has no pixels and reports depth 0.
                            depth: if runtime.window_is_input_only(drawable) { 0 } else { facts.depth },
                            root: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                            geometry: facts.geometry,
                            border_width: runtime.window_border_width(drawable),
                        }),
                        Err(error) => {
                            let mut error = x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(drawable.local.raw()).unwrap_or(0),
                            );
                            if error.code == XErrorCode::BadWindow {
                                error.code = XErrorCode::BadDrawable;
                            }
                            XClientOutput::Error(error)
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::GetImage {
                    drawable,
                    format,
                    x,
                    y,
                    width,
                    height,
                    plane_mask,
                }) => {
                    let region = Rect {
                        x: i32::from(x),
                        y: i32::from(y),
                        width: i32::from(width),
                        height: i32::from(height),
                    };
                    let reply = crate::image::read_drawable_image(
                        runtime,
                        context.namespace,
                        drawable,
                        region,
                        format,
                        plane_mask,
                        context.byte_order,
                    );
                    let outputs = vec![match reply {
                        Ok(readback) => XClientOutput::Reply(XClientReply::GetImage {
                            sequence: context.sequence,
                            depth: readback.depth,
                            visual: readback.visual,
                            data: readback.data,
                        }),
                        Err(error) => XClientOutput::Error(crate::image::image_client_error(
                            context.sequence,
                            context.major_opcode,
                            0,
                            drawable,
                            format,
                            error,
                        )),
                    }];
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::QueryTree { window }) => {
                    let output =
                        match runtime.window_parent_and_children(context.namespace, window) {
                            Ok((parent, children)) => {
                                XClientOutput::Reply(XClientReply::QueryTree {
                                    sequence: context.sequence,
                                    root: XResourceId::new(
                                        u64::from(X_SETUP_DEFAULT_ROOT),
                                        1,
                                    ),
                                    parent,
                                    children,
                                })
                            }
                            Err(error) => XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(window.local.raw()).unwrap_or(0))),
                        };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
        _ => unreachable!("request family checked before dispatch"),
    };
    // A window request may cover or uncover any viewable window of the
    // namespace: every one whose visibility changed is told, after the
    // request's own events (VisibilityNotify after UnmapNotify, XTS Xlib11
    // VisibilityNotify 2; the map's own report is already in place before
    // its Expose, VisibilityNotify 3).
    result.outputs.extend(runtime.visibility_changes(context.namespace).into_iter().map(
        |(window, state)| {
            XClientOutput::Event(XClientEvent::VisibilityNotify {
                sequence: context.sequence,
                window,
                state,
            })
        },
    ));
    Handled(result)
}

fn resolve_window_visual(
    runtime: &XAuthorityRuntime,
    namespace: NamespaceId,
    parent: XResourceId,
    depth: u8,
    visual: u32,
    colormap: Option<XResourceId>,
) -> Result<(u8, u32, XResourceId), (XErrorCode, u32)> {
    let (parent_depth, parent_visual, parent_colormap) =
        if parent.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
            (
                24,
                X_SETUP_DEFAULT_VISUAL,
                XResourceId::new(u64::from(X_SETUP_DEFAULT_COLORMAP), 1),
            )
        } else {
            runtime
                .validate_window_access(namespace, parent)
                .map_err(|_| {
                    (
                        XErrorCode::BadWindow,
                        u32::try_from(parent.local.raw()).unwrap_or(0),
                    )
                })?;
            runtime.window_visual(parent)
        };

    let resolved_depth = if depth == 0 { parent_depth } else { depth };
    let resolved_visual = if visual == 0 { parent_visual } else { visual };
    let advertised = x_true_color_visual(resolved_visual)
        .ok_or((XErrorCode::BadMatch, resolved_visual))?;
    if advertised.depth != resolved_depth {
        return Err((XErrorCode::BadMatch, resolved_visual));
    }

    let copy_parent_colormap = colormap.is_none_or(|value| value.local.raw() == 0);
    if copy_parent_colormap {
        if resolved_visual != parent_visual {
            return Err((XErrorCode::BadMatch, resolved_visual));
        }
        return Ok((resolved_depth, resolved_visual, parent_colormap));
    }

    let resolved_colormap = colormap.expect("an explicit colormap was checked above");
    let colormap_visual = runtime
        .colormap_visual(namespace, resolved_colormap)
        .map_err(|_| {
            (
                XErrorCode::BadColor,
                u32::try_from(resolved_colormap.local.raw()).unwrap_or(0),
            )
        })?;
    if colormap_visual != resolved_visual {
        return Err((
            XErrorCode::BadMatch,
            u32::try_from(resolved_colormap.local.raw()).unwrap_or(0),
        ));
    }
    Ok((resolved_depth, resolved_visual, resolved_colormap))
}

#[allow(clippy::too_many_arguments)]
fn outputs_from_map_response(
    context: XDispatchContext,
    window: XResourceId,
    already_mapped: bool,
    map_state: Option<crate::XMapState>,
    override_redirect: bool,
    input_only: bool,
    visibility: u8,
    response: &XAuthorityResponsePacket,
) -> Vec<XClientOutput> {
    if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
        return vec![XClientOutput::Error(x_error_from_runtime(
            error,
            context.sequence,
            context.major_opcode,
            0,
            u32::try_from(window.local.raw()).unwrap_or(0)))];
    }
    // "When the window is already mapped, then a call to XMapWindow has no
    // effect." No effect includes no events: a client that maps twice was
    // being told twice that the window appeared.
    if already_mapped {
        return Vec::new();
    }
    let Some(crate::XMapState::Unviewable | crate::XMapState::Viewable) = map_state else {
        return Vec::new();
    };
    let mut outputs = vec![XClientOutput::Event(XClientEvent::MapNotify {
        sequence: context.sequence,
        event: window,
        window,
        override_redirect,
    })];
    // "The server does not generate Expose events on windows whose class is
    // specified as InputOnly", nor VisibilityNotify: such a window has no
    // contents to show or hide, only a map state.
    if map_state == Some(crate::XMapState::Viewable) && !input_only {
        // What the map makes visible: a window mapped under another is
        // partially or fully obscured from its first report (XTS Xlib11
        // VisibilityNotify 8 and 9).
        outputs.push(XClientOutput::Event(XClientEvent::VisibilityNotify {
            sequence: context.sequence,
            window,
            state: visibility,
        }));
        if let Some(surface) = response.surfaces.first() {
            outputs.push(XClientOutput::Event(XClientEvent::Expose {
                sequence: context.sequence,
                window,
                x: 0,
                y: 0,
                width: clamp_u16(surface.geometry.width),
                height: clamp_u16(surface.geometry.height),
                count: 0,
            }));
        }
    }
    outputs
}

fn x11_map_state(state: crate::XMapState) -> u8 {
    match state {
        crate::XMapState::Unmapped => 0,
        crate::XMapState::Unviewable => 1,
        crate::XMapState::Viewable => 2,
    }
}

/// Admits the root window alongside a client's own windows.
///
/// The root is synthetic here: it is never inserted into the resource table, so
/// `validate_window_access` cannot find it and refuses it. Requests that name a
/// window purely to scope something -- a grab, a cursor, an event selection --
/// accept the root in X11, and refusing it turns an ordinary client idiom into a
/// `BadWindow`. `validate_drawable_access` already admits the root for the same
/// reason; this is the window-shaped half of that rule.
///
/// Requests that act *on* a window rather than scope to one keep using
/// `validate_window_access` directly, because refusing the root is correct for
/// them: reparenting, destroying, and creating a GLX drawable from the root are
/// all errors.
fn validate_window_or_root_access(
    runtime: &XAuthorityRuntime,
    namespace: NamespaceId,
    window: XResourceId,
) -> Result<(), XAuthorityRuntimeError> {
    if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
        Ok(())
    } else {
        runtime.validate_window_access(namespace, window)
    }
}

/// ColormapNotify's state: the default colormap is the one installed, and
/// the only one ListInstalledColormaps names.
pub(crate) fn colormap_state(colormap: u32) -> u8 {
    u8::from(colormap == crate::X_SETUP_DEFAULT_COLORMAP)
}
