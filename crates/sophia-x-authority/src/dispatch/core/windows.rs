fn dispatch_core_window_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::CreateWindow { .. }
            | XWireRequest::Authority(..)
            | XWireRequest::ChangeWindowAttributes { .. }
            | XWireRequest::GetWindowAttributes { .. }
            | XWireRequest::DestroyWindow { .. }
            | XWireRequest::ReparentWindow { .. }
            | XWireRequest::DestroySubwindows { .. }
            | XWireRequest::MapSubwindows { .. }
            | XWireRequest::UnmapWindow { .. }
            | XWireRequest::ConfigureWindow { .. }
            | XWireRequest::GetGeometry { .. }
            | XWireRequest::GetImage { .. }
            | XWireRequest::QueryTree { .. }
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::CreateWindow {
                    packet,
                    parent,
                    background_pixmap,
                    background_pixel,
                    override_redirect,
                    depth,
                    visual,
                    colormap,
                    input_only,
                    ..
                } => {
                    let kind = packet.kind.clone();
                    let namespace = packet.namespace;
                    let transaction = packet.transaction;
                    let XAuthorityRequestKind::CreateWindow { window, .. } = &kind else {
                        unreachable!("CreateWindow wire requests carry CreateWindow authority packets")
                    };
                    if runtime.resource_id_in_use(*window) {
                        return Handled(XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(crate::XClientError {
                                code: XErrorCode::BadIdChoice,
                                sequence: context.sequence,
                                resource_id: u32::try_from(window.local.raw()).unwrap_or(0),
                                minor_code: 0,
                                major_code: context.major_opcode,
                            })],
                            metadata_candidates: Vec::new(),
                        });
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
                                return Handled(XDispatchResult {
                                    response: None,
                                    outputs: vec![XClientOutput::Error(crate::XClientError {
                                        code,
                                        sequence: context.sequence,
                                        resource_id,
                                        minor_code: 0,
                                        major_code: context.major_opcode,
                                    })],
                                    metadata_candidates: Vec::new(),
                                });
                            }
                        };
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
                    }
                    let mut outputs = outputs_from_authority_response(context, &kind, &response);
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
                            border_width: 0,
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
                    let outputs = if let XAuthorityRequestKind::MapWindow { window, .. } = kind {
                        outputs_from_map_response(
                            context,
                            window,
                            already_mapped,
                            runtime.window_map_state(context.namespace, window).ok(),
                            runtime
                                .window_override_redirect(context.namespace, window)
                                .unwrap_or(false),
                            &response,
                        )
                    } else {
                        outputs_from_authority_response(context, &kind, &response)
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::ChangeWindowAttributes {
                    window,
                    background_pixmap,
                    background_pixel,
                    override_redirect,
                    cursor,
                    ..
                } => {
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
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
                        return Handled(XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                cursor,
                            ))],
                            metadata_candidates: Vec::new(),
                        });
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
                    XDispatchResult {
                        response: override_redirect.map(|_| response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::GetWindowAttributes { window } => {
                    let output = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        XClientOutput::Reply(XClientReply::GetWindowAttributes {
                            sequence: context.sequence,
                            visual: X_SETUP_DEFAULT_VISUAL,
                            colormap: XResourceId::new(u64::from(X_SETUP_DEFAULT_COLORMAP), 1),
                            map_state: 2,
                            override_redirect: false,
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
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::DestroyWindow { window } => {
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
                    let outputs = match runtime
                        .destroy_window_subtree(context.namespace, window)
                    {
                        Ok(destroyed) => {
                            // Destruction order is descendants first, and the
                            // notifications follow it: a client watching a
                            // subtree learns about a child before the parent
                            // that contained it.
                            destroyed
                                .into_iter()
                                .flat_map(|destroyed| {
                                    properties
                                        .remove_window(context.namespace, destroyed.window);
                                    response.removed_surfaces.push(destroyed.surface);
                                    // A mapped window is unmapped as part of
                                    // being destroyed, and that unmap is
                                    // reported in its own right, before the
                                    // destroy. `from_configure` is false: an
                                    // explicit request caused it.
                                    let unmap = (destroyed.was_mapped && destroyed.is_subtree_root).then_some(
                                        XClientOutput::Event(
                                            crate::XClientEvent::UnmapNotify {
                                                sequence: context.sequence,
                                                event: destroyed.window,
                                                window: destroyed.window,
                                                from_configure: false,
                                            },
                                        ),
                                    );
                                    // Addressed to the window itself. The router
                                    // adds the parent-addressed copy for
                                    // SubstructureNotify selectors, the same way
                                    // it does for map and unmap, so both forms
                                    // come from one path.
                                    unmap.into_iter().chain(std::iter::once(
                                        XClientOutput::Event(
                                            crate::XClientEvent::DestroyNotify {
                                                sequence: context.sequence,
                                                event: destroyed.window,
                                                window: destroyed.window,
                                            },
                                        ),
                                    ))
                                })
                                .collect()
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
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::ReparentWindow {
                    window,
                    parent,
                    x,
                    y,
                } => {
                    let transaction = context.transaction;
                    let result = runtime
                        .set_window_parent(context.namespace, window, parent)
                        .and_then(|()| {
                            runtime.configure_window_geometry_observed(
                                context.namespace,
                                window,
                                XWindowGeometryUpdate {
                                    x: Some(x),
                                    y: Some(y),
                                    generation: u64::from(context.sequence),
                                    ..XWindowGeometryUpdate::default()
                                },
                            )
                        });
                    let (response, outputs) = match result {
                        Ok(surface) => {
                            let mut response = XAuthorityResponsePacket::accepted(transaction);
                            response.surfaces.push(surface);
                            (response, Vec::new())
                        }
                        Err(error) => (
                            XAuthorityResponsePacket::rejected(transaction, error),
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0)))],
                        ),
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::DestroySubwindows { window } => {
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
                    let outputs = match runtime
                        .destroy_direct_subwindows(context.namespace, window)
                    {
                        Ok(destroyed) => destroyed
                            .into_iter()
                            .flat_map(|destroyed| {
                                properties.remove_window(context.namespace, destroyed.window);
                                response.removed_surfaces.push(destroyed.surface);
                                // As for a single destroy: a mapped window is
                                // unmapped on its way out, and that is reported
                                // before the destroy.
                                let unmap = (destroyed.was_mapped && destroyed.is_subtree_root).then_some(
                                    XClientOutput::Event(crate::XClientEvent::UnmapNotify {
                                        sequence: context.sequence,
                                        event: destroyed.window,
                                        window: destroyed.window,
                                        from_configure: false,
                                    }),
                                );
                                unmap.into_iter().chain(std::iter::once(
                                    XClientOutput::Event(crate::XClientEvent::DestroyNotify {
                                        sequence: context.sequence,
                                        event: destroyed.window,
                                        window: destroyed.window,
                                    }),
                                ))
                            })
                            .collect(),
                        Err(error) => {
                            response = XAuthorityResponsePacket::rejected(transaction, error);
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0),
                            ))]
                        }
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::MapSubwindows { window } => {
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
                    let outputs = match runtime.map_direct_subwindows(
                        context.namespace,
                        window,
                        u64::from(context.sequence),
                    ) {
                        Ok(surfaces) => {
                            response.surfaces = surfaces;
                            response
                                .surfaces
                                .iter()
                                .flat_map(|surface| {
                                    let window = XResourceId {
                                        local: surface.local_id,
                                    };
                                    let map_state = runtime
                                        .window_map_state(context.namespace, window)
                                        .ok();
                                    if !matches!(
                                        map_state,
                                        Some(crate::XMapState::Unviewable | crate::XMapState::Viewable)
                                    ) {
                                        return Vec::new();
                                    }
                                    let override_redirect = runtime
                                        .window_override_redirect(context.namespace, window)
                                        .unwrap_or(false);
                                    let mut outputs = vec![XClientOutput::Event(
                                        XClientEvent::MapNotify {
                                            sequence: context.sequence,
                                            event: window,
                                            window,
                                            override_redirect,
                                        },
                                    )];
                                    if map_state == Some(crate::XMapState::Viewable) {
                                        outputs.push(XClientOutput::Event(
                                            XClientEvent::VisibilityNotify {
                                            sequence: context.sequence,
                                            window,
                                            state: 0,
                                            },
                                        ));
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
                                    outputs
                                })
                                .collect()
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
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::UnmapWindow { window } => {
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
                    let outputs = match runtime.unmap_window(context.namespace, window) {
                        Ok(Some(surface)) => {
                            response.surfaces.push(surface);
                            // Addressed to the window itself; the router adds
                            // the parent-addressed copy for SubstructureNotify
                            // selectors, as it does for map and destroy.
                            //
                            // `from_configure` is false: this is a client
                            // asking, not a window falling out of view because
                            // an ancestor was reconfigured.
                            vec![XClientOutput::Event(crate::XClientEvent::UnmapNotify {
                                sequence: context.sequence,
                                event: window,
                                window,
                                from_configure: false,
                            })]
                        }
                        // Already unmapped: the request has no effect, and an
                        // event here would report a transition that never
                        // happened.
                        Ok(None) => Vec::new(),
                        Err(error) => {
                            response = XAuthorityResponsePacket::rejected(transaction, error);
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0)))]
                        }
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::ConfigureWindow {
                    window,
                    x,
                    y,
                    width,
                    height,
                    sibling,
                    stack_mode,
                    ..
                } => {
                    let before = runtime.window_geometry(context.namespace, window).ok();
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
                    let outputs = if let Err(error) = configure {
                        vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(window.local.raw()).unwrap_or(0)))]
                    } else {
                        match runtime.window_geometry(context.namespace, window) {
                            Ok(geometry) if before != Some(geometry) || !client_controls => {
                                let override_redirect = runtime
                                    .window_override_redirect(context.namespace, window)
                                    .unwrap_or(false);
                                vec![XClientOutput::Event(XClientEvent::ConfigureNotify {
                                    sequence: context.sequence,
                                    synthetic: !client_controls,
                                    event: window,
                                    window,
                                    above_sibling: None,
                                    x: clamp_i16(geometry.x),
                                    y: clamp_i16(geometry.y),
                                    width: clamp_u16(geometry.width),
                                    height: clamp_u16(geometry.height),
                                    border_width: 0,
                                    override_redirect,
                                })]
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
                    XDispatchResult {
                        response: restacked.map(|surface| {
                            let mut response = XAuthorityResponsePacket::accepted(
                                context.transaction,
                            );
                            response.surfaces.push(surface);
                            response
                        }),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::GetGeometry { drawable } => {
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
                            depth: facts.depth,
                            root: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                            geometry: facts.geometry,
                            border_width: 0,
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
                XWireRequest::GetImage {
                    drawable,
                    format,
                    x,
                    y,
                    width,
                    height,
                    plane_mask,
                } => {
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
                XWireRequest::QueryTree { window } => {
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
    })
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

fn outputs_from_map_response(
    context: XDispatchContext,
    window: XResourceId,
    already_mapped: bool,
    map_state: Option<crate::XMapState>,
    override_redirect: bool,
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
    if map_state == Some(crate::XMapState::Viewable) {
        outputs.push(XClientOutput::Event(XClientEvent::VisibilityNotify {
            sequence: context.sequence,
            window,
            state: 0,
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
