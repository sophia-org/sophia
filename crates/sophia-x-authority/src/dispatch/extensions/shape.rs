/// The protocol error a shape refusal maps to.
fn shape_error_code(error: crate::XShapeError) -> XErrorCode {
    match error {
        crate::XShapeError::UnknownWindow => XErrorCode::BadWindow,
        crate::XShapeError::UnknownPixmap => XErrorCode::BadPixmap,
        crate::XShapeError::InvalidValue => XErrorCode::BadValue,
        // A mask that is not one bit deep cannot describe a shape; that is a
        // mismatch between the argument and the request rather than a bad id.
        crate::XShapeError::NotABitmap => XErrorCode::BadMatch,
    }
}

fn dispatch_shape_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
        XWireRequest::Shape(crate::XShapeRequest::ShapeQueryVersion)
            | XWireRequest::Shape(crate::XShapeRequest::ShapeRectangles { .. })
            | XWireRequest::Shape(crate::XShapeRequest::ShapeMask { .. })
            | XWireRequest::Shape(crate::XShapeRequest::ShapeCombine { .. })
            | XWireRequest::Shape(crate::XShapeRequest::ShapeOffset { .. })
            | XWireRequest::Shape(crate::XShapeRequest::ShapeQueryExtents { .. })
            | XWireRequest::Shape(crate::XShapeRequest::ShapeSelectInput { .. })
            | XWireRequest::Shape(crate::XShapeRequest::ShapeInputSelected { .. })
            | XWireRequest::Shape(crate::XShapeRequest::ShapeGetRectangles { .. })
            | XWireRequest::Shape(crate::XShapeRequest::ShapeUnimplemented { .. })
    ) {
        return Unhandled(request);
    }

    let error_output = |error: crate::XShapeError, minor: u8, resource: crate::XResourceId| {
        XClientOutput::Error(crate::XClientError {
            code: shape_error_code(error),
            sequence: context.sequence,
            resource_id: u32::try_from(resource.local.raw()).unwrap_or(0),
            minor_code: u16::from(minor),
            major_code: context.major_opcode,
        })
    };
    // A change becomes an event; no change becomes nothing at all. Window
    // managers re-assert shapes constantly, and a notify per re-assertion is
    // what broke panel buttons in the implementation this gating came from.
    let change_output = |change: Option<crate::XShapeChange>| -> Vec<XClientOutput> {
        change
            .map(|change| {
                XClientOutput::Event(XClientEvent::ShapeNotify {
                    sequence: context.sequence,
                    kind: change.kind,
                    window: change.window,
                    extents: change.extents,
                    shaped: change.shaped,
                })
            })
            .into_iter()
            .collect()
    };

    Handled(match request {
        XWireRequest::Shape(crate::XShapeRequest::ShapeQueryVersion) => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Reply(XClientReply::ShapeQueryVersion {
                sequence: context.sequence,
                major_version: crate::X_SHAPE_MAJOR_VERSION,
                minor_version: crate::X_SHAPE_MINOR_VERSION,
            })],
            metadata_candidates: Vec::new(),
        },
        XWireRequest::Shape(crate::XShapeRequest::ShapeRectangles {
            op,
            kind,
            ordering,
            destination,
            x_offset,
            y_offset,
            rectangles,
        }) => {
            // Every ordering is accepted and none is trusted: the list is
            // canonicalised on arrival, so a client that mislabels its own
            // ordering still gets the shape it drew.
            if ordering > crate::X_SHAPE_ORDERING_YX_BANDED {
                return Handled(XDispatchResult {
                    response: None,
                    outputs: vec![error_output(
                        crate::XShapeError::InvalidValue,
                        crate::X_SHAPE_RECTANGLES_MINOR_OPCODE,
                        destination,
                    )],
                    metadata_candidates: Vec::new(),
                });
            }
            let source = sophia_protocol::geometry::region_algebra::translate(
                &rectangles,
                i32::from(x_offset),
                i32::from(y_offset),
            );
            match runtime.combine_shape_region(context.namespace, destination, kind, op, source) {
                Ok(change) => XDispatchResult {
                    response: shape_republish(context, runtime, change),
                    outputs: change_output(change),
                    metadata_candidates: Vec::new(),
                },
                Err(error) => XDispatchResult {
                    response: None,
                    outputs: vec![error_output(
                        error,
                        crate::X_SHAPE_RECTANGLES_MINOR_OPCODE,
                        destination,
                    )],
                    metadata_candidates: Vec::new(),
                },
            }
        }
        XWireRequest::Shape(crate::XShapeRequest::ShapeMask {
            op,
            kind,
            destination,
            x_offset,
            y_offset,
            source,
        }) => {
            let outcome = match source {
                Some(pixmap) => runtime
                    .shape_mask_rects(context.namespace, pixmap)
                    .and_then(|rects| {
                        let moved = sophia_protocol::geometry::region_algebra::translate(
                            &rects,
                            i32::from(x_offset),
                            i32::from(y_offset),
                        );
                        runtime.combine_shape_region(context.namespace, destination, kind, op, moved)
                    }),
                // No mask with Set returns the kind to its default, so it
                // tracks the window's geometry again. With any other
                // operation the source is that same default region, which is
                // what the protocol says an absent mask means.
                None if op == crate::X_SHAPE_OP_SET => {
                    runtime.reset_shape(context.namespace, destination, kind)
                }
                None => {
                    let (_, default) = runtime.effective_shape(destination, kind);
                    runtime.combine_shape_region(
                        context.namespace,
                        destination,
                        kind,
                        op,
                        default,
                    )
                }
            };
            match outcome {
                Ok(change) => XDispatchResult {
                    response: shape_republish(context, runtime, change),
                    outputs: change_output(change),
                    metadata_candidates: Vec::new(),
                },
                Err(error) => XDispatchResult {
                    response: None,
                    outputs: vec![error_output(
                        error,
                        crate::X_SHAPE_MASK_MINOR_OPCODE,
                        source.unwrap_or(destination),
                    )],
                    metadata_candidates: Vec::new(),
                },
            }
        }
        XWireRequest::Shape(crate::XShapeRequest::ShapeCombine {
            op,
            kind,
            source_kind,
            destination,
            x_offset,
            y_offset,
            source,
        }) => {
            let outcome = if !XAuthorityRuntime::shape_kind_is_valid_public(source_kind) {
                Err(crate::XShapeError::InvalidValue)
            } else if runtime
                .validate_window_access(context.namespace, source)
                .is_err()
            {
                Err(crate::XShapeError::UnknownWindow)
            } else {
                let (_, rects) = runtime.effective_shape(source, source_kind);
                let moved = sophia_protocol::geometry::region_algebra::translate(
                    &rects,
                    i32::from(x_offset),
                    i32::from(y_offset),
                );
                runtime.combine_shape_region(context.namespace, destination, kind, op, moved)
            };
            match outcome {
                Ok(change) => XDispatchResult {
                    response: shape_republish(context, runtime, change),
                    outputs: change_output(change),
                    metadata_candidates: Vec::new(),
                },
                Err(error) => XDispatchResult {
                    response: None,
                    outputs: vec![error_output(
                        error,
                        crate::X_SHAPE_COMBINE_MINOR_OPCODE,
                        destination,
                    )],
                    metadata_candidates: Vec::new(),
                },
            }
        }
        XWireRequest::Shape(crate::XShapeRequest::ShapeOffset {
            kind,
            destination,
            x_offset,
            y_offset,
        }) => match runtime.offset_shape(
            context.namespace,
            destination,
            kind,
            i32::from(x_offset),
            i32::from(y_offset),
        ) {
            Ok(change) => XDispatchResult {
                response: shape_republish(context, runtime, change),
                outputs: change_output(change),
                metadata_candidates: Vec::new(),
            },
            Err(error) => XDispatchResult {
                response: None,
                outputs: vec![error_output(
                    error,
                    crate::X_SHAPE_OFFSET_MINOR_OPCODE,
                    destination,
                )],
                metadata_candidates: Vec::new(),
            },
        },
        XWireRequest::Shape(crate::XShapeRequest::ShapeQueryExtents { window }) => {
            let outputs = if runtime
                .validate_window_access(context.namespace, window)
                .is_err()
            {
                vec![error_output(
                    crate::XShapeError::UnknownWindow,
                    crate::X_SHAPE_QUERY_EXTENTS_MINOR_OPCODE,
                    window,
                )]
            } else {
                let (bounding_shaped, bounding) =
                    runtime.effective_shape(window, crate::X_SHAPE_KIND_BOUNDING);
                let (clip_shaped, clip) = runtime.effective_shape(window, crate::X_SHAPE_KIND_CLIP);
                vec![XClientOutput::Reply(XClientReply::ShapeQueryExtents {
                    sequence: context.sequence,
                    bounding_shaped,
                    clip_shaped,
                    bounding_extents: sophia_protocol::geometry::region_algebra::extents(&bounding)
                        .unwrap_or_default(),
                    clip_extents: sophia_protocol::geometry::region_algebra::extents(&clip)
                        .unwrap_or_default(),
                })]
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::Shape(crate::XShapeRequest::ShapeSelectInput { window, enable }) => XDispatchResult {
            response: None,
            outputs: runtime
                .select_shape_input(context.namespace, context.client_id, window, enable)
                .err()
                .map(|error| {
                    error_output(error, crate::X_SHAPE_SELECT_INPUT_MINOR_OPCODE, window)
                })
                .into_iter()
                .collect(),
            metadata_candidates: Vec::new(),
        },
        XWireRequest::Shape(crate::XShapeRequest::ShapeInputSelected { window }) => {
            let outputs =
                match runtime.shape_input_selected(context.namespace, context.client_id, window) {
                    Ok(enabled) => {
                        vec![XClientOutput::Reply(XClientReply::ShapeInputSelected {
                            sequence: context.sequence,
                            enabled,
                        })]
                    }
                    Err(error) => vec![error_output(
                        error,
                        crate::X_SHAPE_INPUT_SELECTED_MINOR_OPCODE,
                        window,
                    )],
                };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::Shape(crate::XShapeRequest::ShapeGetRectangles { window, kind }) => {
            let outputs = if !XAuthorityRuntime::shape_kind_is_valid_public(kind) {
                vec![error_output(
                    crate::XShapeError::InvalidValue,
                    crate::X_SHAPE_GET_RECTANGLES_MINOR_OPCODE,
                    window,
                )]
            } else if runtime
                .validate_window_access(context.namespace, window)
                .is_err()
            {
                vec![error_output(
                    crate::XShapeError::UnknownWindow,
                    crate::X_SHAPE_GET_RECTANGLES_MINOR_OPCODE,
                    window,
                )]
            } else {
                let (_, rects) = runtime.effective_shape(window, kind);
                vec![XClientOutput::Reply(XClientReply::ShapeGetRectangles {
                    sequence: context.sequence,
                    // The store keeps regions canonical, so claiming
                    // YX-banded is a claim this server can honour rather
                    // than one it repeats from the request.
                    ordering: crate::X_SHAPE_ORDERING_YX_BANDED,
                    rects: sophia_protocol::geometry::region_algebra::canonicalize(&rects),
                })]
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        // No version of SHAPE defines a minor above eight.
        XWireRequest::Shape(crate::XShapeRequest::ShapeUnimplemented { minor_opcode }) => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Error(crate::XClientError {
                code: if minor_opcode <= crate::X_SHAPE_LAST_MINOR_OPCODE {
                    XErrorCode::BadImplementation
                } else {
                    XErrorCode::BadRequest
                },
                sequence: context.sequence,
                resource_id: 0,
                minor_code: u16::from(minor_opcode),
                major_code: context.major_opcode,
            })],
            metadata_candidates: Vec::new(),
        },
        other => return Unhandled(other),
    })
}

/// Republish a window whose bounding shape moved, so the new outline reaches
/// the screen without waiting for the client to draw again.
///
/// Only bounding changes the pixels. Clip is consumed by nothing here, and
/// input shapes never affect what is composed.
fn shape_republish(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    change: Option<crate::XShapeChange>,
) -> Option<XAuthorityResponsePacket> {
    let change = change?;
    if change.kind != crate::X_SHAPE_KIND_BOUNDING {
        return None;
    }
    runtime.republish_shaped_window(context.transaction, context.namespace, change.window)
}
