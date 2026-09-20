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
                return Handled(core_resource_validation_error(
                    context,
                    error,
                    XErrorCode::BadPixmap,
                    mask,
                ));
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
            if let Err(error) = runtime.graphics_context_values(context.namespace, gc) {
                return Handled(core_resource_validation_error(
                    context,
                    error,
                    XErrorCode::BadGraphicsContext,
                    gc,
                ));
            }
            if let Some(mask) = values.clip_mask
                && let Err(error) = runtime.validate_clip_mask(context.namespace, mask)
            {
                return Handled(core_resource_validation_error(
                    context,
                    error,
                    XErrorCode::BadPixmap,
                    mask,
                ));
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
            window,
            x,
            y,
            width,
            height,
            ..
        } => {
            let transaction = context.transaction;
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
            let response = match runtime.window_background_pixel(context.namespace, window) {
                Ok(pixel) => runtime.apply_clear_with_pixel(
                    transaction,
                    context.namespace,
                    window,
                    Region::single(Rect {
                        x: i32::from(x),
                        y: i32::from(y),
                        width: clear_width,
                        height: clear_height,
                    }),
                    pixel,
                ),
                Err(error) => XAuthorityResponsePacket::rejected(transaction, error),
            };
            let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    0,
                    u32::try_from(window.local.raw()).unwrap_or(0),
                ))]
            } else {
                Vec::new()
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
        } => {
            if runtime.resource_id_in_use(cursor) {
                return Handled(core_resource_bad_id_choice(context, cursor));
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
        } => {
            if runtime.resource_id_in_use(cursor) {
                return Handled(core_resource_bad_id_choice(context, cursor));
            }
            let outputs = if let Err(error) =
                runtime.validate_font_access(context.namespace, source_font)
            {
                vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    0,
                    u32::try_from(source_font.local.raw()).unwrap_or(0),
                ))]
            } else if let Some(mask_font) = mask_font {
                if let Err(error) = runtime.validate_font_access(context.namespace, mask_font) {
                    vec![XClientOutput::Error(x_error_from_runtime(
                        error,
                        context.sequence,
                        context.major_opcode,
                        0,
                        u32::try_from(mask_font.local.raw()).unwrap_or(0),
                    ))]
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
                Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    0,
                    u32::try_from(cursor.local.raw()).unwrap_or(0),
                ))],
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
                Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    0,
                    u32::try_from(cursor.local.raw()).unwrap_or(0),
                ))],
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
            let outputs = match runtime.copy_graphics_context(
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

