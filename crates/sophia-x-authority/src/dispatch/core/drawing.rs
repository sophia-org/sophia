fn dispatch_core_drawing_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
    _properties: &mut XPropertyTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
        XWireRequest::Core(crate::XCoreRequest::PolyFillRectangle { .. })
            | XWireRequest::Core(crate::XCoreRequest::CopyArea { .. })
            | XWireRequest::Core(crate::XCoreRequest::PolyLine { .. })
            | XWireRequest::Core(crate::XCoreRequest::PolyRectangle { .. })
            | XWireRequest::Core(crate::XCoreRequest::PolySegment { .. })
            | XWireRequest::Core(crate::XCoreRequest::PolyFillArc { .. })
            | XWireRequest::Core(crate::XCoreRequest::PolyArc { .. })
            | XWireRequest::Core(crate::XCoreRequest::PolyPoint { .. })
            | XWireRequest::Core(crate::XCoreRequest::CopyPlane { .. })
            | XWireRequest::Core(crate::XCoreRequest::PolyText8 { .. })
            | XWireRequest::Core(crate::XCoreRequest::ImageText8 { .. })
            | XWireRequest::Core(crate::XCoreRequest::PolyText16 { .. })
            | XWireRequest::Core(crate::XCoreRequest::ImageText16 { .. })
            | XWireRequest::Core(crate::XCoreRequest::FillPoly { .. })
            | XWireRequest::Core(crate::XCoreRequest::PutImage { .. })
    ) {
        return Unhandled(request);
    }
    Handled(match request {
        XWireRequest::Core(crate::XCoreRequest::PolyFillRectangle {
            drawable,
            gc,
            rectangles,
        }) => {
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
            let response = runtime.draw_through_inferiors(transaction, context.namespace, drawable, values.subwindow_mode, |runtime| runtime.apply_core_draw_with_gc(
                transaction,
                context.namespace,
                drawable,
                damage,
                &values,
            ));
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
        XWireRequest::Core(crate::XCoreRequest::PolyRectangle {
            drawable,
            gc,
            rectangles,
        }) => {
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, drawable, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            let response = runtime.draw_through_inferiors(transaction, context.namespace, drawable, values.subwindow_mode, |runtime| runtime.apply_rectangle_draw(
                transaction,
                context.namespace,
                drawable,
                &rectangles,
                &values,
            ));
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
        XWireRequest::Core(crate::XCoreRequest::CopyArea {
            source,
            destination,
            gc,
            src_x,
            src_y,
            dst_x,
            dst_y,
            width,
            height,
        }) => {
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
            let response = runtime.draw_through_inferiors(transaction, context.namespace, destination, values.subwindow_mode, |runtime| runtime.apply_copy_area_with_gc(
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
            ));
            let outputs = match response.outcome {
                XAuthorityResponseOutcome::Accepted if values.graphics_exposures => {
                    copy_exposure_events(
                        context,
                        runtime,
                        source,
                        destination,
                        Rect {
                            x: i32::from(src_x),
                            y: i32::from(src_y),
                            width: i32::from(width),
                            height: i32::from(height),
                        },
                        (i32::from(dst_x), i32::from(dst_y)),
                        &values,
                    )
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
        XWireRequest::Core(crate::XCoreRequest::PolyLine {
            drawable,
            gc,
            points,
        }) => {
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, drawable, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            let response = runtime.draw_through_inferiors(transaction, context.namespace, drawable, values.subwindow_mode, |runtime| runtime.apply_line_draw(
                transaction,
                context.namespace,
                drawable,
                &points,
                &values,
            ));
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
        XWireRequest::Core(crate::XCoreRequest::PolySegment {
            drawable,
            gc,
            segments,
        }) => {
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
            let response = runtime.draw_through_inferiors(transaction, context.namespace, drawable, values.subwindow_mode, |runtime| runtime.apply_segment_draw(
                transaction,
                context.namespace,
                drawable,
                &segments,
                &values,
            ));
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
        XWireRequest::Core(crate::XCoreRequest::PolyFillArc { drawable, gc, arcs }) => {
            // `miPolyFillArc`: the pixels whose centres lie inside the
            // ellipse, clipped to a pie slice or a chord as the graphics
            // context's arc mode says.
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
            let spans = crate::software::geometry::fill_arc::fill(&arcs, pie_slice);
            core_rectangle_fill(context, runtime, drawable, &spans, &values)
        }
        XWireRequest::Core(crate::XCoreRequest::PolyArc { drawable, gc, arcs }) => {
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, drawable, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            // `miPolyArc`, at every width and line style.
            let response = runtime.draw_through_inferiors(transaction, context.namespace, drawable, values.subwindow_mode, |runtime| runtime.apply_arc_draw(
                transaction,
                context.namespace,
                drawable,
                &arcs,
                &values,
            ));
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
        XWireRequest::Core(crate::XCoreRequest::CopyPlane {
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
        }) => {
            let transaction = context.transaction;
            let values = match core_draw_gc(context, runtime, destination, gc) {
                Ok(values) => values,
                Err((error, code, resource)) => {
                    return Handled(core_draw_validation_error(
                        context, transaction, error, code, resource,
                    ));
                }
            };
            // The source may be of any depth, but the plane must lie within
            // it. The decoder has already refused zero and several bits, so
            // what is left is a plane at or above the source depth.
            let source_depth = match runtime.drawable_depth(context.namespace, source) {
                Ok(depth) => depth,
                Err(error) => {
                    return Handled(core_draw_validation_error(
                        context,
                        transaction,
                        error,
                        XErrorCode::BadDrawable,
                        source,
                    ));
                }
            };
            if u64::from(bit_plane) >= 1u64 << source_depth {
                return Handled(XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Error(crate::XClientError {
                        code: XErrorCode::BadValue,
                        sequence: context.sequence,
                        resource_id: bit_plane,
                        minor_code: 0,
                        major_code: context.major_opcode,
                    })],
                    metadata_candidates: Vec::new(),
                });
            }
            let response = runtime.draw_through_inferiors(transaction, context.namespace, destination, values.subwindow_mode, |runtime| runtime.apply_copy_plane(
                transaction,
                context.namespace,
                source,
                destination,
                (i32::from(src_x), i32::from(src_y)),
                (i32::from(dst_x), i32::from(dst_y)),
                (i32::from(width), i32::from(height)),
                bit_plane,
                &values,
            ));
            let outputs = match response.outcome {
                XAuthorityResponseOutcome::Accepted if values.graphics_exposures => {
                    copy_exposure_events(
                        context,
                        runtime,
                        source,
                        destination,
                        Rect {
                            x: i32::from(src_x),
                            y: i32::from(src_y),
                            width: i32::from(width),
                            height: i32::from(height),
                        },
                        (i32::from(dst_x), i32::from(dst_y)),
                        &values,
                    )
                }
                XAuthorityResponseOutcome::Accepted => Vec::new(),
                XAuthorityResponseOutcome::Rejected(error) => {
                    vec![XClientOutput::Error(x_error_from_runtime(
                        error,
                        context.sequence,
                        context.major_opcode,
                        0,
                        u32::try_from(source.local.raw()).unwrap_or(0),
                    ))]
                }
            };
            XDispatchResult {
                response: Some(response),
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::Core(crate::XCoreRequest::PolyPoint {
            drawable,
            gc,
            coordinate_mode,
            points,
        }) => {
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
        XWireRequest::Core(crate::XCoreRequest::PolyText8 {
            drawable,
            gc,
            x,
            y,
            items,
        })
        | XWireRequest::Core(crate::XCoreRequest::PolyText16 {
            drawable,
            gc,
            x,
            y,
            items,
        }) => dispatch_poly_text(context, runtime, drawable, gc, x, y, &items),
        XWireRequest::Core(crate::XCoreRequest::ImageText8 {
            drawable,
            gc,
            x,
            y,
            text,
        }) => {
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
        XWireRequest::Core(crate::XCoreRequest::ImageText16 {
            drawable,
            gc,
            x,
            y,
            chars,
        }) => dispatch_text_draw(
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
        XWireRequest::Core(crate::XCoreRequest::FillPoly {
            drawable,
            gc,
            shape,
            coordinate_mode,
            points,
        }) => {
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
            // `miFillPolygon`: the convex filler when the client declares
            // the polygon Convex, the general one for Complex or Nonconvex.
            let spans = if shape == X_SHAPE_CONVEX {
                crate::software::geometry::polygon::convex(&points)
            } else {
                let winding = values.fill_rule == crate::X_FILL_WINDING;
                crate::software::geometry::polygon::general(&points, winding)
            };
            core_rectangle_fill(context, runtime, drawable, &spans, &values)
        }
        XWireRequest::Core(crate::XCoreRequest::PutImage {
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
        }) => {
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
            let response = runtime.draw_through_inferiors(transaction, context.namespace, drawable, gc_values.subwindow_mode, |runtime| runtime.apply_put_image(
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
            ));
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

/// The events a copy owes a context with graphics-exposures: one
/// GraphicsExpose per destination rectangle whose source lay outside the
/// source drawable, counted down to zero, or one NoExpose when the whole
/// source was there. The destination keeps its old pixels under an exposed
/// rectangle, so the client must repaint it itself.
fn copy_exposure_events(
    context: XDispatchContext,
    runtime: &XAuthorityRuntime,
    source: XResourceId,
    destination: XResourceId,
    src: Rect,
    (dst_x, dst_y): (i32, i32),
    values: &crate::XGraphicsContextValues,
) -> Vec<XClientOutput> {
    let exposed = graphics_exposed_rects(
        runtime,
        context.namespace,
        source,
        destination,
        src,
        (dst_x, dst_y),
        values,
    );
    if exposed.is_empty() {
        return vec![XClientOutput::Event(XClientEvent::NoExpose {
            sequence: context.sequence,
            drawable: destination,
            minor_opcode: 0,
            major_opcode: context.major_opcode,
        })];
    }
    let total = exposed.len();
    exposed
        .into_iter()
        .enumerate()
        .map(|(index, rect)| {
            XClientOutput::Event(XClientEvent::GraphicsExpose {
                sequence: context.sequence,
                drawable: destination,
                x: clamp_u16(rect.x),
                y: clamp_u16(rect.y),
                width: clamp_u16(rect.width),
                height: clamp_u16(rect.height),
                minor_opcode: 0,
                count: u16::try_from(total - index - 1).unwrap_or(u16::MAX),
                major_opcode: context.major_opcode,
            })
        })
        .collect()
}

/// The destination rectangles a copy could not fill, in destination
/// coordinates: the part of the source rectangle outside the source
/// drawable, moved over the destination and cut to what the destination and
/// the context's clip list admit. Empty when the whole source was there.
fn graphics_exposed_rects(
    runtime: &XAuthorityRuntime,
    namespace: NamespaceId,
    source: XResourceId,
    destination: XResourceId,
    src: Rect,
    (dst_x, dst_y): (i32, i32),
    values: &crate::XGraphicsContextValues,
) -> Vec<Rect> {
    let (Some(source_bounds), Some(destination_bounds)) = (
        drawable_bounds(runtime, namespace, source),
        drawable_bounds(runtime, namespace, destination),
    ) else {
        return Vec::new();
    };
    let available = rect_intersection(src, source_bounds);
    if available == Some(src) {
        return Vec::new();
    }
    let missing = match available {
        Some(available) => rect_difference(src, available),
        None => vec![src],
    };
    let clip: Vec<Rect> = match &values.clip_rectangles {
        Some(rectangles) => rectangles
            .iter()
            .map(|rectangle| Rect {
                x: rectangle.x + i32::from(values.clip_x_origin),
                y: rectangle.y + i32::from(values.clip_y_origin),
                width: rectangle.width,
                height: rectangle.height,
            })
            .filter_map(|rectangle| rect_intersection(rectangle, destination_bounds))
            .collect(),
        None => vec![destination_bounds],
    };
    missing
        .into_iter()
        .map(|rect| Rect {
            x: rect.x + dst_x - src.x,
            y: rect.y + dst_y - src.y,
            width: rect.width,
            height: rect.height,
        })
        .flat_map(|rect| {
            clip.iter()
                .filter_map(move |clip| rect_intersection(rect, *clip))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A drawable's own rectangle, at its own origin. None for a drawable the
/// runtime holds no size for, such as the root.
fn drawable_bounds(
    runtime: &XAuthorityRuntime,
    namespace: NamespaceId,
    drawable: XResourceId,
) -> Option<Rect> {
    let size = runtime
        .pixmap_size(namespace, drawable)
        .ok()
        .or_else(|| {
            runtime
                .window_geometry(namespace, drawable)
                .ok()
                .map(|geometry| sophia_protocol::Size {
                    width: geometry.width,
                    height: geometry.height,
                })
        })?;
    Some(Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    })
}

fn rect_intersection(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then_some(Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

/// `outer` less `inner`, for an `inner` lying within `outer`: the band
/// above, the pieces beside, then the band below, which is the order a
/// banded region lists them in.
fn rect_difference(outer: Rect, inner: Rect) -> Vec<Rect> {
    let outer_right = outer.x + outer.width;
    let outer_bottom = outer.y + outer.height;
    let inner_right = inner.x + inner.width;
    let inner_bottom = inner.y + inner.height;
    let mut bands = Vec::new();
    if inner.y > outer.y {
        bands.push(Rect {
            x: outer.x,
            y: outer.y,
            width: outer.width,
            height: inner.y - outer.y,
        });
    }
    if inner.x > outer.x {
        bands.push(Rect {
            x: outer.x,
            y: inner.y,
            width: inner.x - outer.x,
            height: inner.height,
        });
    }
    if inner_right < outer_right {
        bands.push(Rect {
            x: inner_right,
            y: inner.y,
            width: outer_right - inner_right,
            height: inner.height,
        });
    }
    if inner_bottom < outer_bottom {
        bands.push(Rect {
            x: outer.x,
            y: inner_bottom,
            width: outer.width,
            height: outer_bottom - inner_bottom,
        });
    }
    bands
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

/// Paint a list of rectangles, reporting the drawable's own errors.
fn core_rectangle_fill(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    drawable: XResourceId,
    rectangles: &[Rect],
    values: &crate::XGraphicsContextValues,
) -> XDispatchResult {
    let response = runtime.draw_through_inferiors(context.transaction, context.namespace, drawable, values.subwindow_mode, |runtime| runtime.apply_span_fill(
        context.transaction,
        context.namespace,
        drawable,
        rectangles,
        values,
    ));
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

/// FillPoly's shape hint for a polygon the client says is convex.
const X_SHAPE_CONVEX: u8 = 2;
