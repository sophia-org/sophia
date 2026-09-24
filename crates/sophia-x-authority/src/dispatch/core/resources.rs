fn dispatch_core_resource_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
    _properties: &mut XPropertyTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
        XWireRequest::CreateGraphicsContext { .. }
            | XWireRequest::ChangeGraphicsContext { .. }
            | XWireRequest::SetClipRectangles { .. }
            | XWireRequest::FreeGraphicsContext { .. }
            | XWireRequest::ClearArea { .. }
            | XWireRequest::OpenFont { .. }
            | XWireRequest::CloseFont { .. }
            | XWireRequest::QueryFont { .. }
            | XWireRequest::CreateCursor { .. }
            | XWireRequest::CreateGlyphCursor { .. }
            | XWireRequest::FreeCursor { .. }
            | XWireRequest::RecolorCursor { .. }
            | XWireRequest::ListFonts { .. }
            | XWireRequest::ListFontsWithInfo { .. }
            | XWireRequest::QueryTextExtents { .. }
            | XWireRequest::SetFontPath
            | XWireRequest::ChangeSaveSet { .. }
            | XWireRequest::SetCloseDownMode { .. }
            | XWireRequest::KillClient { .. }
            | XWireRequest::GetFontPath
            | XWireRequest::CopyGraphicsContext { .. }
            | XWireRequest::SetDashes { .. }
            | XWireRequest::CreatePixmap { .. }
            | XWireRequest::FreePixmap { .. }
    ) {
        return Unhandled(request);
    }
    Handled(match request {
        XWireRequest::CreateGraphicsContext {
            gc,
            drawable,
            values,
        } => {
            if runtime.resource_id_in_use(gc) {
                return Handled(core_resource_bad_id_choice(context, gc));
            }
            if let Err(error) = runtime.validate_drawable_access(context.namespace, drawable) {
                return Handled(core_resource_validation_error(
                    context,
                    error,
                    XErrorCode::BadDrawable,
                    drawable,
                ));
            }
            // A clip mask must be a real depth-one pixmap; anything else is
            // the client's error rather than a limit of this server.
            if let Some(mask) = values.clip_mask
                && let Err(error) = runtime.validate_clip_mask(context.namespace, mask)
            {
                // A pixmap of another depth is a Match error; a name that is
                // no pixmap is a Pixmap error.
                let code = if error == XAuthorityRuntimeError::InvalidSurface {
                    XErrorCode::BadMatch
                } else {
                    XErrorCode::BadPixmap
                };
                return Handled(core_resource_validation_error(context, error, code, mask));
            }
            if let Some(font) = values.font
                && let Err(error) = runtime.validate_font_access(context.namespace, font)
            {
                return Handled(core_resource_validation_error(
                    context,
                    error,
                    XErrorCode::BadFont,
                    font,
                ));
            }
            let depth = runtime
                .drawable_depth(context.namespace, drawable)
                .unwrap_or(0);
            if let Err(refusal) = validate_gc_pattern_pixmaps(context, runtime, depth, &values) {
                return Handled(refusal);
            }
            let outputs = runtime
                .create_graphics_context(context.namespace, gc, drawable, values)
                .err()
                .map(|error| {
                    XClientOutput::Error(x_error_from_runtime(
                        error,
                        context.sequence,
                        context.major_opcode,
                        0,
                        u32::try_from(gc.local.raw()).unwrap_or(0),
                    ))
                })
                .into_iter()
                .collect();
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::ChangeGraphicsContext {
            gc,
            value_mask,
            values,
        } => {
            let depth = match runtime.graphics_context_depth_and_values(context.namespace, gc) {
                Ok((depth, _)) => depth,
                Err(error) => {
                    return Handled(core_resource_validation_error(
                        context,
                        error,
                        XErrorCode::BadGraphicsContext,
                        gc,
                    ));
                }
            };
            if let Some(mask) = values.clip_mask
                && let Err(error) = runtime.validate_clip_mask(context.namespace, mask)
            {
                // A pixmap of another depth is a Match error; a name that is
                // no pixmap is a Pixmap error.
                let code = if error == XAuthorityRuntimeError::InvalidSurface {
                    XErrorCode::BadMatch
                } else {
                    XErrorCode::BadPixmap
                };
                return Handled(core_resource_validation_error(context, error, code, mask));
            }
            if let Err(refusal) = validate_gc_pattern_pixmaps(context, runtime, depth, &values) {
                return Handled(refusal);
            }
            if value_mask & (1 << 14) != 0 {
                let font = values.font.unwrap_or(XResourceId::new(0, 1));
                if let Err(error) = runtime.validate_font_access(context.namespace, font) {
                    return Handled(core_resource_validation_error(
                        context,
                        error,
                        XErrorCode::BadFont,
                        font,
                    ));
                }
            }
            let outputs = runtime
                .change_graphics_context(context.namespace, gc, value_mask, values)
                .err()
                .map(|error| {
                    XClientOutput::Error(x_error_from_runtime(
                        error,
                        context.sequence,
                        context.major_opcode,
                        0,
                        u32::try_from(gc.local.raw()).unwrap_or(0),
                    ))
                })
                .into_iter()
                .collect();
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::SetClipRectangles {
            gc,
            clip_x_origin,
            clip_y_origin,
            rectangles,
        } => {
            if let Err(error) = runtime.set_graphics_context_clip_rectangles(
                context.namespace,
                gc,
                clip_x_origin,
                clip_y_origin,
                rectangles,
            ) {
                return Handled(core_resource_validation_error(
                    context,
                    error,
                    XErrorCode::BadGraphicsContext,
                    gc,
                ));
            }
            XDispatchResult {
                response: None,
                outputs: Vec::new(),
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::FreeGraphicsContext { gc } => {
            if let Err(error) = runtime.free_graphics_context(context.namespace, gc) {
                return Handled(core_resource_validation_error(
                    context,
                    error,
                    XErrorCode::BadGraphicsContext,
                    gc,
                ));
            }
            XDispatchResult {
                response: None,
                outputs: Vec::new(),
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::ClearArea {
            exposures,
            window,
            x,
            y,
            width,
            height,
        } => {
            let transaction = context.transaction;
            // An InputOnly window has no background to restore.
            if runtime.window_is_input_only(window) {
                return Handled(core_resource_validation_error(
                    context,
                    XAuthorityRuntimeError::InvalidSurface,
                    XErrorCode::BadMatch,
                    window,
                ));
            }
            let geometry = runtime.window_geometry(context.namespace, window).ok();
            let clear_width = if width == 0 {
                geometry
                    .map(|geometry| geometry.width.saturating_sub(i32::from(x)).max(0))
                    .unwrap_or(0)
            } else {
                i32::from(width)
            };
            let clear_height = if height == 0 {
                geometry
                    .map(|geometry| geometry.height.saturating_sub(i32::from(y)).max(0))
                    .unwrap_or(0)
            } else {
                i32::from(height)
            };
            let area = Rect {
                x: i32::from(x),
                y: i32::from(y),
                width: clear_width,
                height: clear_height,
            };
            let response = match runtime.window_background_pixel(context.namespace, window) {
                Ok(pixel) => runtime.apply_clear_with_pixel(
                    transaction,
                    context.namespace,
                    window,
                    Region::single(area),
                    pixel,
                ),
                Err(error) => XAuthorityResponsePacket::rejected(transaction, error),
            };
            let outputs = match response.outcome {
                XAuthorityResponseOutcome::Rejected(error) => {
                    vec![XClientOutput::Error(x_error_from_runtime(
                        error,
                        context.sequence,
                        context.major_opcode,
                        0,
                        u32::try_from(window.local.raw()).unwrap_or(0),
                    ))]
                }
                XAuthorityResponseOutcome::Accepted if exposures => geometry
                    .and_then(|geometry| clear_area_exposure(context, runtime, window, geometry, area))
                    .map(XClientOutput::Event)
                    .into_iter()
                    .collect(),
                XAuthorityResponseOutcome::Accepted => Vec::new(),
            };
            XDispatchResult {
                response: Some(response),
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::OpenFont { font, name } => {
            if runtime.resource_id_in_use(font) {
                return Handled(core_resource_bad_id_choice(context, font));
            }
            // The catalog resolves the name against the configured path and
            // the built-in element. A name no element publishes is BadName,
            // which is what the client expects and what lets a toolkit probe
            // for a face without dying.
            let outputs = match runtime.open_named_font(
                context.namespace,
                font,
                &name,
                u64::from(context.sequence),
            ) {
                Ok(()) => Vec::new(),
                Err(crate::XFontOpenFailure::Unresolved) => {
                    tracing::debug!(
                        font = font.local.raw(),
                        "sophia_x11_font schema=1 status=refused"
                    );
                    vec![XClientOutput::Error(crate::XClientError {
                        code: XErrorCode::BadName,
                        sequence: context.sequence,
                        resource_id: u32::try_from(font.local.raw()).unwrap_or(0),
                        minor_code: 0,
                        major_code: context.major_opcode,
                    })]
                }
                Err(crate::XFontOpenFailure::Resource(error)) => {
                    vec![XClientOutput::Error(x_error_from_runtime(
                        error,
                        context.sequence,
                        context.major_opcode,
                        0,
                        u32::try_from(font.local.raw()).unwrap_or(0),
                    ))]
                }
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::CloseFont { font } => {
            let outputs = match runtime.close_font(context.namespace, font) {
                Ok(()) => Vec::new(),
                Err(error) => {
                    core_resource_validation_error(context, error, XErrorCode::BadFont, font)
                        .outputs
                }
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::QueryFont { font } => {
            let output = match runtime.fontable_face(context.namespace, font) {
                Ok(face) => XClientOutput::Reply(XClientReply::QueryFont {
                    sequence: context.sequence,
                    metrics: Box::new(face.metrics.clone()),
                }),
                Err(error) => {
                    core_resource_validation_error(context, error, XErrorCode::BadFont, font)
                        .outputs
                        .into_iter()
                        .next()
                        .expect("resource error has one output")
                }
            };
            XDispatchResult {
                response: None,
                outputs: vec![output],
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::CreateCursor {
            cursor,
            source,
            mask,
            hotspot_x,
            hotspot_y,
        } => {
            if runtime.resource_id_in_use(cursor) {
                return Handled(core_resource_bad_id_choice(context, cursor));
            }
            if let Some(error) =
                cursor_bitmap_error(runtime, context, source, mask, (hotspot_x, hotspot_y))
            {
                return Handled(XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Error(error)],
                    metadata_candidates: Vec::new(),
                });
            }
            let result = runtime
                .validate_drawable_access(context.namespace, source)
                .and_then(|()| {
                    mask.map_or(Ok(()), |mask| {
                        runtime.validate_drawable_access(context.namespace, mask)
                    })
                })
                .and_then(|()| {
                    runtime.create_cursor(context.namespace, cursor, u64::from(context.sequence))
                });
            let outputs = result
                .err()
                .map(|error| {
                    XClientOutput::Error(x_error_from_runtime(
                        error,
                        context.sequence,
                        context.major_opcode,
                        0,
                        u32::try_from(cursor.local.raw()).unwrap_or(0),
                    ))
                })
                .into_iter()
                .collect();
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::CreateGlyphCursor {
            cursor,
            source_font,
            mask_font,
            source_char,
            mask_char,
        } => {
            if runtime.resource_id_in_use(cursor) {
                return Handled(core_resource_bad_id_choice(context, cursor));
            }
            // A character the font does not define is a Value error, once
            // the font itself is known to exist.
            let undefined = |font, char: u16| {
                runtime.font_face(context.namespace, font).is_ok_and(|face| {
                    let [byte1, byte2] = char.to_be_bytes();
                    face.metrics.char_info(byte1, byte2).is_none()
                })
            };
            if undefined(source_font, source_char)
                || mask_font.is_some_and(|font| undefined(font, mask_char))
            {
                return Handled(XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Error(crate::XClientError {
                        code: XErrorCode::BadValue,
                        sequence: context.sequence,
                        resource_id: u32::from(source_char),
                        minor_code: 0,
                        major_code: context.major_opcode,
                    })],
                    metadata_candidates: Vec::new(),
                });
            }
            // A font that does not exist is a Font error, whatever the
            // runtime calls an unknown resource.
            let bad_font = |font: crate::XResourceId| {
                vec![XClientOutput::Error(crate::XClientError {
                    code: XErrorCode::BadFont,
                    sequence: context.sequence,
                    resource_id: u32::try_from(font.local.raw()).unwrap_or(0),
                    minor_code: 0,
                    major_code: context.major_opcode,
                })]
            };
            let outputs = if runtime
                .validate_font_access(context.namespace, source_font)
                .is_err()
            {
                bad_font(source_font)
            } else if let Some(mask_font) = mask_font {
                if runtime
                    .validate_font_access(context.namespace, mask_font)
                    .is_err()
                {
                    bad_font(mask_font)
                } else {
                    match runtime.create_cursor(
                        context.namespace,
                        cursor,
                        u64::from(context.sequence),
                    ) {
                        Ok(()) => Vec::new(),
                        Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(cursor.local.raw()).unwrap_or(0),
                        ))],
                    }
                }
            } else {
                match runtime.create_cursor(context.namespace, cursor, u64::from(context.sequence))
                {
                    Ok(()) => Vec::new(),
                    Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                        error,
                        context.sequence,
                        context.major_opcode,
                        0,
                        u32::try_from(cursor.local.raw()).unwrap_or(0),
                    ))],
                }
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::FreeCursor { cursor } => {
            let outputs = match runtime.free_cursor(context.namespace, cursor) {
                Ok(()) => Vec::new(),
                Err(error) => vec![XClientOutput::Error(cursor_error(error, context, cursor))],
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::RecolorCursor { cursor } => {
            let outputs = match runtime.validate_cursor_access(context.namespace, cursor) {
                Ok(()) => Vec::new(),
                Err(error) => vec![XClientOutput::Error(cursor_error(error, context, cursor))],
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::ListFonts {
            max_names,
            ref pattern,
        } => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Reply(XClientReply::ListFonts {
                sequence: context.sequence,
                names: runtime.list_fonts(pattern, usize::from(max_names)),
            })],
            metadata_candidates: Vec::new(),
        },
        XWireRequest::ListFontsWithInfo {
            max_names,
            ref pattern,
        } => {
            // Each name is reported with its own metrics, so every entry costs
            // a load. The bound is smaller than the plain listing's because
            // this one measures rather than names.
            let names = runtime.list_fonts_with_info(
                pattern,
                usize::from(max_names).min(crate::X_LIST_FONTS_WITH_INFO_MAX_NAMES),
            );
            XDispatchResult {
                response: None,
                outputs: vec![XClientOutput::Reply(XClientReply::ListFontsWithInfo {
                    sequence: context.sequence,
                    names,
                })],
                metadata_candidates: Vec::new(),
            }
        }
        // The font path is session configuration. Refusing a client's attempt
        // to change it is the safeguard that lets a host path be exposed at
        // all: nothing a client sends can add a directory to search.
        XWireRequest::CopyGraphicsContext {
            source,
            destination,
            value_mask,
        } => {
            // Both contexts must exist, and then they must share a depth: a
            // context is bound to the depth of the drawable it was made for,
            // and copying components across depths is a Match error.
            let depths = runtime
                .graphics_context_depth_and_values(context.namespace, source)
                .map_err(|error| (error, source))
                .and_then(|(source_depth, _)| {
                    runtime
                        .graphics_context_depth_and_values(context.namespace, destination)
                        .map(|(destination_depth, _)| (source_depth, destination_depth))
                        .map_err(|error| (error, destination))
                });
            let outputs = match depths {
                Err((error, gc)) => {
                    core_resource_validation_error(
                        context,
                        error,
                        XErrorCode::BadGraphicsContext,
                        gc,
                    )
                    .outputs
                }
                Ok((source_depth, destination_depth)) if source_depth != destination_depth => {
                    core_resource_validation_error(
                        context,
                        XAuthorityRuntimeError::InvalidSurface,
                        XErrorCode::BadMatch,
                        destination,
                    )
                    .outputs
                }
                Ok(_) => match runtime.copy_graphics_context(
                    context.namespace,
                    source,
                    destination,
                    value_mask,
                ) {
                    Ok(()) => Vec::new(),
                    Err(error) => {
                        core_resource_validation_error(
                            context,
                            error,
                            XErrorCode::BadGraphicsContext,
                            destination,
                        )
                        .outputs
                    }
                },
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::SetDashes {
            gc,
            dash_offset,
            ref dashes,
        } => {
            // The server's own order: an unknown graphics context is reported
            // before the pattern is judged, and only then is an empty or
            // zero-length pattern a bad value. A dash of zero length would
            // never advance.
            let outputs = if dashes.is_empty() || dashes.contains(&0) {
                if let Err(error) = runtime.validate_graphics_context(context.namespace, gc) {
                    core_resource_validation_error(
                        context,
                        error,
                        XErrorCode::BadGraphicsContext,
                        gc,
                    )
                    .outputs
                } else {
                    vec![XClientOutput::Error(crate::XClientError {
                        code: XErrorCode::BadValue,
                        sequence: context.sequence,
                        resource_id: 0,
                        minor_code: 0,
                        major_code: context.major_opcode,
                    })]
                }
            } else {
                match runtime.set_graphics_context_dashes(
                    context.namespace,
                    gc,
                    dash_offset,
                    dashes,
                ) {
                    Ok(()) => Vec::new(),
                    Err(error) => {
                        core_resource_validation_error(
                            context,
                            error,
                            XErrorCode::BadGraphicsContext,
                            gc,
                        )
                        .outputs
                    }
                }
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        // Validated here, acted on by the socket layer, which owns the
        // leases: a window in the requester's own range is refused
        // (BadMatch: a client saves another's windows, not its own), an
        // unknown one BadWindow.
        XWireRequest::ChangeSaveSet { window, own_window, .. } => {
            let error = |code: XErrorCode| {
                XClientOutput::Error(crate::XClientError {
                    code,
                    sequence: context.sequence,
                    resource_id: u32::try_from(window.local.raw()).unwrap_or(0),
                    minor_code: 0,
                    major_code: context.major_opcode,
                })
            };
            let outputs = if window.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT)
                || runtime
                    .validate_window_access(context.namespace, window)
                    .is_err()
            {
                vec![error(XErrorCode::BadWindow)]
            } else if own_window {
                vec![error(XErrorCode::BadMatch)]
            } else {
                Vec::new()
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::SetCloseDownMode { .. } | XWireRequest::KillClient { .. } => XDispatchResult {
            response: None,
            outputs: Vec::new(),
            metadata_candidates: Vec::new(),
        },
        XWireRequest::SetFontPath => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Error(crate::XClientError {
                code: XErrorCode::BadAccess,
                sequence: context.sequence,
                resource_id: 0,
                minor_code: 0,
                major_code: context.major_opcode,
            })],
            metadata_candidates: Vec::new(),
        },
        XWireRequest::GetFontPath => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Reply(XClientReply::GetFontPath {
                sequence: context.sequence,
                directories: runtime.font_path_names(),
            })],
            metadata_candidates: Vec::new(),
        },
        XWireRequest::QueryTextExtents { fontable, ref chars } => {
            let output = match runtime.fontable_face(context.namespace, fontable) {
                Ok(face) => XClientOutput::Reply(XClientReply::QueryTextExtents {
                    sequence: context.sequence,
                    extents: face.metrics.text_extents(chars),
                }),
                Err(error) => {
                    core_resource_validation_error(context, error, XErrorCode::BadFont, fontable)
                        .outputs
                        .into_iter()
                        .next()
                        .expect("resource error has one output")
                }
            };
            XDispatchResult {
                response: None,
                outputs: vec![output],
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::CreatePixmap {
            depth,
            pixmap,
            drawable,
            width,
            height,
        } => {
            if runtime.resource_id_in_use(pixmap) {
                return Handled(core_resource_bad_id_choice(context, pixmap));
            }
            let outputs =
                if let Err(error) = runtime.validate_drawable_access(context.namespace, drawable) {
                    return Handled(core_resource_validation_error(
                        context,
                        error,
                        XErrorCode::BadDrawable,
                        drawable,
                    ));
                } else if width == 0 || height == 0 {
                    vec![XClientOutput::Error(crate::XClientError {
                        code: XErrorCode::BadValue,
                        sequence: context.sequence,
                        resource_id: 0,
                        minor_code: 0,
                        major_code: context.major_opcode,
                    })]
                } else if crate::x11_pixmap_format(depth).is_none() {
                    vec![XClientOutput::Error(crate::XClientError {
                        code: XErrorCode::BadValue,
                        sequence: context.sequence,
                        resource_id: u32::from(depth),
                        minor_code: 0,
                        major_code: context.major_opcode,
                    })]
                } else if let Err(error) = runtime.create_pixmap(
                    context.namespace,
                    pixmap,
                    sophia_protocol::Size {
                        width: i32::from(width),
                        height: i32::from(height),
                    },
                    depth,
                    u64::from(context.sequence),
                ) {
                    vec![XClientOutput::Error(x_error_from_runtime(
                        error,
                        context.sequence,
                        context.major_opcode,
                        0,
                        u32::try_from(pixmap.local.raw()).unwrap_or(0),
                    ))]
                } else {
                    Vec::new()
                };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::FreePixmap { pixmap } => {
            if let Err(error) = runtime.free_pixmap(context.namespace, pixmap) {
                return Handled(core_resource_validation_error(
                    context,
                    error,
                    XErrorCode::BadPixmap,
                    pixmap,
                ));
            }
            XDispatchResult {
                response: None,
                outputs: Vec::new(),
                metadata_candidates: Vec::new(),
            }
        }
        _ => unreachable!("request family checked before dispatch"),
    })
}

fn core_resource_bad_id_choice(
    context: XDispatchContext,
    resource: XResourceId,
) -> XDispatchResult {
    XDispatchResult {
        response: None,
        outputs: vec![XClientOutput::Error(crate::XClientError {
            code: XErrorCode::BadIdChoice,
            sequence: context.sequence,
            resource_id: u32::try_from(resource.local.raw()).unwrap_or(0),
            minor_code: 0,
            major_code: context.major_opcode,
        })],
        metadata_candidates: Vec::new(),
    }
}

/// The tile and stipple a context names must be pixmaps this client may
/// use: a tile of the context's depth and a stipple of depth one. The
/// protocol names Pixmap for an unknown one and Match for the wrong depth.
fn validate_gc_pattern_pixmaps(
    context: XDispatchContext,
    runtime: &XAuthorityRuntime,
    depth: u8,
    values: &crate::XGraphicsContextValues,
) -> Result<(), XDispatchResult> {
    for (pixmap, required_depth) in [(values.tile, depth), (values.stipple, 1)] {
        let Some(pixmap) = pixmap else {
            continue;
        };
        if let Err(error) = runtime.validate_pixmap_access(context.namespace, pixmap) {
            return Err(core_resource_validation_error(
                context,
                error,
                XErrorCode::BadPixmap,
                pixmap,
            ));
        }
        if runtime.drawable_depth(context.namespace, pixmap) != Ok(required_depth) {
            return Err(core_resource_validation_error(
                context,
                XAuthorityRuntimeError::InvalidSurface,
                XErrorCode::BadMatch,
                pixmap,
            ));
        }
    }
    Ok(())
}

/// The Expose a ClearArea owes when the client asked for exposures: the
/// cleared rectangle within a viewable window, in one event. Nothing is
/// retained for an unviewable window, so nothing is reported for it.
fn clear_area_exposure(
    context: XDispatchContext,
    runtime: &XAuthorityRuntime,
    window: XResourceId,
    geometry: Rect,
    area: Rect,
) -> Option<XClientEvent> {
    if runtime.window_map_state(context.namespace, window) != Ok(crate::XMapState::Viewable) {
        return None;
    }
    let bounds = Rect {
        x: 0,
        y: 0,
        width: geometry.width,
        height: geometry.height,
    };
    let exposed = rect_intersection(area, bounds)?;
    Some(XClientEvent::Expose {
        sequence: context.sequence,
        window,
        x: clamp_u16(exposed.x),
        y: clamp_u16(exposed.y),
        width: clamp_u16(exposed.width),
        height: clamp_u16(exposed.height),
        count: 0,
    })
}

fn core_resource_validation_error(
    context: XDispatchContext,
    runtime_error: XAuthorityRuntimeError,
    missing_resource_code: XErrorCode,
    resource: XResourceId,
) -> XDispatchResult {
    let resource_id = u32::try_from(resource.local.raw()).unwrap_or(0);
    let code = match runtime_error {
        XAuthorityRuntimeError::InvalidResource
        | XAuthorityRuntimeError::UnknownResource
        | XAuthorityRuntimeError::WrongResourceKind
        | XAuthorityRuntimeError::InvalidSurface => missing_resource_code,
        _ => {
            x_error_from_runtime(
                runtime_error,
                context.sequence,
                context.major_opcode,
                0,
                resource_id,
            )
            .code
        }
    };
    XDispatchResult {
        response: None,
        outputs: vec![XClientOutput::Error(crate::XClientError {
            code,
            sequence: context.sequence,
            resource_id,
            minor_code: 0,
            major_code: context.major_opcode,
        })],
        metadata_candidates: Vec::new(),
    }
}

/// Why a core cursor's bitmaps cannot make a cursor, if they cannot.
///
/// The source and the mask are depth-one pixmaps, a mask is the source's
/// size, and the hotspot lies on the source, or at most one past its edge as
/// in Xorg; anything else is a Match error.
/// A name that is no pixmap at all is a Pixmap error.
fn cursor_bitmap_error(
    runtime: &XAuthorityRuntime,
    context: XDispatchContext,
    source: crate::XResourceId,
    mask: Option<crate::XResourceId>,
    (hotspot_x, hotspot_y): (u16, u16),
) -> Option<crate::XClientError> {
    let error = |code, resource: crate::XResourceId| crate::XClientError {
        code,
        sequence: context.sequence,
        resource_id: u32::try_from(resource.local.raw()).unwrap_or(0),
        minor_code: 0,
        major_code: context.major_opcode,
    };
    let Ok((size, depth)) = runtime.pixmap_geometry(context.namespace, source) else {
        return Some(error(XErrorCode::BadPixmap, source));
    };
    // Xorg admits a hotspot one past each edge (`x > width` in dix), and
    // clients written against it may rely on that, so this does too.
    if depth != 1 || i32::from(hotspot_x) > size.width || i32::from(hotspot_y) > size.height {
        return Some(error(XErrorCode::BadMatch, source));
    }
    let mask = mask?;
    match runtime.pixmap_geometry(context.namespace, mask) {
        Err(_) => Some(error(XErrorCode::BadPixmap, mask)),
        Ok((mask_size, mask_depth)) if mask_depth != 1 || mask_size != size => {
            Some(error(XErrorCode::BadMatch, mask))
        }
        Ok(_) => None,
    }
}

/// The error for a cursor that cannot be used: a name that is no cursor is
/// a Cursor error, which the runtime's generic mapping would call a Window.
fn cursor_error(
    error: XAuthorityRuntimeError,
    context: XDispatchContext,
    cursor: crate::XResourceId,
) -> crate::XClientError {
    let mut mapped = x_error_from_runtime(
        error,
        context.sequence,
        context.major_opcode,
        0,
        u32::try_from(cursor.local.raw()).unwrap_or(0),
    );
    if matches!(
        error,
        XAuthorityRuntimeError::UnknownResource
            | XAuthorityRuntimeError::InvalidResource
            | XAuthorityRuntimeError::WrongResourceKind
    ) {
        mapped.code = XErrorCode::BadCursor;
    }
    mapped
}
