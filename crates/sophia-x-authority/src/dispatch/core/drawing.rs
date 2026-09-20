fn dispatch_core_drawing_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
    _properties: &mut XPropertyTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
        XWireRequest::PolyFillRectangle { .. }
            | XWireRequest::CopyArea { .. }
            | XWireRequest::PolyLine { .. }
            | XWireRequest::PolyRectangle { .. }
            | XWireRequest::PolySegment { .. }
            | XWireRequest::PolyFillArc { .. }
            | XWireRequest::PolyArc { .. }
            | XWireRequest::PolyPoint { .. }
            | XWireRequest::CopyPlane { .. }
            | XWireRequest::PolyText8 { .. }
            | XWireRequest::ImageText8 { .. }
            | XWireRequest::PolyText16 { .. }
            | XWireRequest::ImageText16 { .. }
            | XWireRequest::FillPoly { .. }
            | XWireRequest::PutImage { .. }
    ) {
        return Unhandled(request);
    }
    Handled(match request {
        XWireRequest::PolyFillRectangle {
            drawable,
            gc,
            rectangles,
        } => {
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, drawable, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            let mut damage = Region::empty();
            for rectangle in rectangles {
                damage.push(rectangle);
            }
            let response = runtime.apply_core_draw_with_gc(
                transaction,
                context.namespace,
                drawable,
                damage,
                &values,
            );
            let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    0,
                    u32::try_from(drawable.local.raw()).unwrap_or(0)))]
            } else {
                Vec::new()
            };
            XDispatchResult {
                response: Some(response),
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::PolyRectangle {
            drawable,
            gc,
            rectangles,
        } => {
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, drawable, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            let response = runtime.apply_rectangle_draw(
                transaction,
                context.namespace,
                drawable,
                &rectangles,
                &values,
            );
            let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    0,
                    u32::try_from(drawable.local.raw()).unwrap_or(0)))]
            } else {
                Vec::new()
            };
            XDispatchResult {
                response: Some(response),
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::CopyArea {
            source,
            destination,
            gc,
            src_x,
            src_y,
            dst_x,
            dst_y,
            width,
            height,
        } => {
            let transaction = context.transaction;
            if let Err(error) = runtime.validate_drawable_access(context.namespace, source) {
                return Handled(core_draw_validation_error(
                    context,
                    transaction,
                    error,
                    XErrorCode::BadDrawable,
                    source,
                ));
            }
            if let Err(error) = runtime.validate_drawable_access(context.namespace, destination) {
                return Handled(core_draw_validation_error(
                    context,
                    transaction,
                    error,
                    XErrorCode::BadDrawable,
                    destination,
                ));
            }
            let (gc_depth, values) =
                match runtime.graphics_context_depth_and_values(context.namespace, gc) {
                    Ok(record) => record,
                    Err(error) => {
                        return Handled(core_draw_validation_error(
                            context,
                            transaction,
                            error,
                            XErrorCode::BadGraphicsContext,
                            gc,
                        ));
                    }
                };
            let source_depth = runtime.drawable_depth(context.namespace, source);
            let destination_depth = runtime.drawable_depth(context.namespace, destination);
            if source_depth != destination_depth || destination_depth != Ok(gc_depth) {
                return Handled(core_draw_validation_error(
                    context,
                    transaction,
                    XAuthorityRuntimeError::InvalidSurface,
                    XErrorCode::BadMatch,
                    destination,
                ));
            }
            let response = runtime.apply_copy_area_with_gc(
                transaction,
                context.namespace,
                source,
                destination,
                src_x,
                src_y,
                dst_x,
                dst_y,
                width,
                height,
                &values,
            );
            let outputs = match response.outcome {
                XAuthorityResponseOutcome::Accepted if values.graphics_exposures => {
                    vec![XClientOutput::Event(XClientEvent::NoExpose {
                        sequence: context.sequence,
                        drawable: destination,
                        minor_opcode: 0,
                        major_opcode: context.major_opcode,
                    })]
                }
                XAuthorityResponseOutcome::Accepted => Vec::new(),
                XAuthorityResponseOutcome::Rejected(error) => {
                    vec![XClientOutput::Error(x_error_from_runtime(
                        error,
                        context.sequence,
                        context.major_opcode,
                        0,
                        u32::try_from(destination.local.raw()).unwrap_or(0)))]
                }
            };
            XDispatchResult {
                response: Some(response),
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::PolyLine {
            drawable,
            gc,
            points,
        } => {
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, drawable, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            let response = runtime.apply_line_draw(
                transaction,
                context.namespace,
                drawable,
                &points,
                &values,
            );
            let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    0,
                    u32::try_from(drawable.local.raw()).unwrap_or(0)))]
            } else {
                Vec::new()
            };
            XDispatchResult {
                response: Some(response),
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::PolySegment {
            drawable,
            gc,
            segments,
        } => {
            // Each segment is its own two-point line: disjoint, so the ends do
            // not join. xterm draws the VT100 line-drawing characters with
            // this request when the font has no glyph for them, which is why
            // recording damage without painting left the box characters
            // missing from a terminal that otherwise looked right.
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, drawable, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            let response = runtime.apply_segment_draw(
                transaction,
                context.namespace,
                drawable,
                &segments,
                &values,
            );
            let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    0,
                    u32::try_from(drawable.local.raw()).unwrap_or(0),
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
        XWireRequest::PolyFillArc { drawable, gc, arcs } => {
            // A filled arc is the polygon its curve encloses, closed through
            // the centre or straight across as the graphics context's arc mode
            // says. Until now this recorded damage and painted nothing.
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, drawable, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            let pie_slice = values.arc_mode == crate::X_ARC_PIE_SLICE;
            let polygons: Vec<Vec<crate::XPoint>> = arcs
                .iter()
                .map(|arc| crate::software::geometry::arc::fill_polygon(*arc, pie_slice))
                .collect();
            core_polygon_draw(context, runtime, drawable, &polygons, &values, true)
        }
        XWireRequest::PolyArc { drawable, gc, arcs } => {
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, drawable, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            // Stroked as the polyline the curve traces, so a stroked arc and a
            // filled one are built from the same points and cannot disagree.
            let segments: Vec<(crate::XPoint, crate::XPoint)> = arcs
                .iter()
                .flat_map(|arc| {
                    let points = crate::software::geometry::arc::polyline(*arc);
                    points
                        .windows(2)
                        .map(|pair| (pair[0], pair[1]))
                        .collect::<Vec<_>>()
                })
                .collect();
            core_segment_draw(context, runtime, drawable, &segments, &values)
        }
        XWireRequest::CopyPlane {
            source,
            destination,
            gc,
            src_x,
            src_y,
            dst_x,
            dst_y,
            width,
            height,
            bit_plane,
        } => {
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, destination, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            let response = runtime.apply_copy_plane(
                transaction,
                context.namespace,
                source,
                destination,
                (i32::from(src_x), i32::from(src_y)),
                (i32::from(dst_x), i32::from(dst_y)),
                (i32::from(width), i32::from(height)),
                bit_plane,
                &values,
            );
            let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    0,
                    u32::try_from(source.local.raw()).unwrap_or(0),
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
        XWireRequest::PolyPoint {
            drawable,
            gc,
            coordinate_mode,
            points,
        } => {
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, drawable, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            let points = crate::wire::absolute_points(&points, coordinate_mode);
            // A point is a one-pixel rectangle, so it goes through the same
            // fill as everything else and inherits clip and function.
            let rectangles: Vec<Rect> = points
                .iter()
                .map(|point| Rect {
                    x: i32::from(point.x),
                    y: i32::from(point.y),
                    width: 1,
                    height: 1,
                })
                .collect();
            core_rectangle_fill(context, runtime, drawable, &rectangles, &values)
        }
        XWireRequest::PolyText8 {
            drawable,
            gc,
            x,
            y,
            items,
        }
        | XWireRequest::PolyText16 {
            drawable,
            gc,
            x,
            y,
            items,
        } => dispatch_poly_text(context, runtime, drawable, gc, x, y, &items),
        XWireRequest::ImageText8 {
            drawable,
            gc,
            x,
            y,
            text,
        } => {
            // An 8-bit request names characters whose high byte is zero, which
            // is how the server reads it against a two-byte face as well.
            let chars: Vec<u16> = text.iter().map(|byte| u16::from(*byte)).collect();
            dispatch_text_draw(
                context,
                runtime,
                drawable,
                gc,
                XTextDraw {
                    x: i32::from(x),
                    baseline: i32::from(y),
                    text: &chars,
                    image: true,
                    font: crate::builtin_font_handle(),
                },
            )
        }
        XWireRequest::ImageText16 {
            drawable,
            gc,
            x,
            y,
            chars,
        } => dispatch_text_draw(
            context,
            runtime,
            drawable,
            gc,
            XTextDraw {
                x: i32::from(x),
                baseline: i32::from(y),
                text: &chars,
                image: true,
                // Replaced by the graphics context's own face before drawing;
                // this only has to be a face.
                font: crate::builtin_font_handle(),
            },
        ),
        XWireRequest::FillPoly {
            drawable,
            gc,
            coordinate_mode,
            points,
            ..
        } => {
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, drawable, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            let points = crate::wire::absolute_points(&points, coordinate_mode);
            let winding = values.fill_rule == crate::X_FILL_WINDING;
            core_polygon_draw(context, runtime, drawable, &[points], &values, winding)
        }
        XWireRequest::PutImage {
            format,
            drawable,
            gc,
            width,
            height,
            dst_x,
            dst_y,
            left_pad,
            depth,
            data,
        } => {
            let transaction = context.transaction;
            if let Err(error) = runtime.validate_drawable_access(context.namespace, drawable) {
                return Handled(core_draw_validation_error(
                    context,
                    transaction,
                    error,
                    XErrorCode::BadDrawable,
                    drawable,
                ));
            }
            let (gc_depth, gc_values) =
                match runtime.graphics_context_depth_and_values(context.namespace, gc) {
                    Ok(record) => record,
                    Err(error) => {
                        return Handled(core_draw_validation_error(
                            context,
                            transaction,
                            error,
                            XErrorCode::BadGraphicsContext,
                            gc,
                        ));
                    }
                };
            if runtime.drawable_depth(context.namespace, drawable) != Ok(gc_depth)
                || (format == 0 && depth != 1)
                || (format != 0 && depth != gc_depth)
            {
                return Handled(core_draw_validation_error(
                    context,
                    transaction,
                    XAuthorityRuntimeError::InvalidSurface,
                    XErrorCode::BadMatch,
                    drawable,
                ));
            }
            let damage = Region::single(Rect {
                x: i32::from(dst_x),
                y: i32::from(dst_y),
                width: i32::from(width),
                height: i32::from(height),
            });
            let pixels = match crate::image::decode_upload(
                format,
                depth,
                width,
                height,
                left_pad,
                context.byte_order,
                &gc_values,
                &data,
            ) {
                Ok(pixels) => pixels,
                Err(code) => {
                    return Handled(core_draw_validation_error(
                        context,
                        transaction,
                        XAuthorityRuntimeError::InvalidResource,
                        code,
                        drawable,
                    ));
                }
            };
            if width == 0 || height == 0 {
                return Handled(XDispatchResult {
                    response: Some(XAuthorityResponsePacket::accepted(transaction)),
                    outputs: Vec::new(),
                    metadata_candidates: Vec::new(),
                });
            }
            let response = runtime.apply_put_image(
                transaction,
                context.namespace,
                drawable,
                damage,
                Some(&pixels),
                Some(&XPutImageSemantics {
                    format,
                    depth: gc_depth,
                    left_pad,
                    byte_order: XByteOrder::LittleEndian,
                    gc: gc_values,
                }),
            );
            let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    0,
                    u32::try_from(drawable.local.raw()).unwrap_or(0)))]
            } else {
                Vec::new()
            };
            XDispatchResult {
                response: Some(response),
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        _ => unreachable!("request family checked before dispatch"),
    })
}

fn core_draw_validation_error(
    context: XDispatchContext,
    transaction: TransactionId,
    runtime_error: XAuthorityRuntimeError,
    missing_resource_code: XErrorCode,
    resource: XResourceId,
) -> XDispatchResult {
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
                u32::try_from(resource.local.raw()).unwrap_or(0))
            .code
        }
    };
    XDispatchResult {
        response: Some(XAuthorityResponsePacket::rejected(
            transaction,
            runtime_error,
        )),
        outputs: vec![XClientOutput::Error(crate::XClientError {
            code,
            sequence: context.sequence,
            resource_id: u32::try_from(resource.local.raw()).unwrap_or(0),
            minor_code: 0,
            major_code: context.major_opcode,
        })],
        metadata_candidates: Vec::new(),
    }
}

fn core_draw_gc(
    context: XDispatchContext,
    runtime: &XAuthorityRuntime,
    drawable: XResourceId,
    gc: XResourceId,
) -> Result<crate::XGraphicsContextValues, (XAuthorityRuntimeError, XErrorCode, XResourceId)> {
    runtime
        .validate_drawable_access(context.namespace, drawable)
        .map_err(|error| (error, XErrorCode::BadDrawable, drawable))?;
    let (depth, values) = runtime
        .graphics_context_depth_and_values(context.namespace, gc)
        .map_err(|error| (error, XErrorCode::BadGraphicsContext, gc))?;
    if runtime.drawable_depth(context.namespace, drawable) != Ok(depth) {
        return Err((
            XAuthorityRuntimeError::InvalidSurface,
            XErrorCode::BadMatch,
            drawable,
        ));
    }
    Ok(values)
}

/// Fill polygons by scanline and paint the spans through the ordinary fill.
fn core_polygon_draw(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    drawable: XResourceId,
    polygons: &[Vec<crate::XPoint>],
    values: &crate::XGraphicsContextValues,
    winding: bool,
) -> XDispatchResult {
    let spans: Vec<Rect> = polygons
        .iter()
        .flat_map(|points| crate::software::geometry::polygon::fill(points, winding))
        .collect();
    core_rectangle_fill(context, runtime, drawable, &spans, values)
}

/// Paint a list of rectangles, reporting the drawable's own errors.
fn core_rectangle_fill(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    drawable: XResourceId,
    rectangles: &[Rect],
    values: &crate::XGraphicsContextValues,
) -> XDispatchResult {
    let response = runtime.apply_span_fill(
        context.transaction,
        context.namespace,
        drawable,
        rectangles,
        values,
    );
    let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
        vec![XClientOutput::Error(x_error_from_runtime(
            error,
            context.sequence,
            context.major_opcode,
            0,
            u32::try_from(drawable.local.raw()).unwrap_or(0),
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

/// Stroke disjoint segments, reporting the drawable's own errors.
fn core_segment_draw(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    drawable: XResourceId,
    segments: &[(crate::XPoint, crate::XPoint)],
    values: &crate::XGraphicsContextValues,
) -> XDispatchResult {
    let response = runtime.apply_segment_draw(
        context.transaction,
        context.namespace,
        drawable,
        segments,
        values,
    );
    let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
        vec![XClientOutput::Error(x_error_from_runtime(
            error,
            context.sequence,
            context.major_opcode,
            0,
            u32::try_from(drawable.local.raw()).unwrap_or(0),
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
