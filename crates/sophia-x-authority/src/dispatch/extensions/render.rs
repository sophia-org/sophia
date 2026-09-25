/// The protocol version each RENDER minor entered at, as a minor-version
/// number, or `None` for a value no version of the protocol ever defined.
///
/// This is what splits the two refusal tiers: a minor gated at or below the
/// advertised version answers `BadImplementation` -- the request exists here
/// and is not offered -- while one gated above it answers `BadRequest`,
/// because a genuine server of the advertised version had no dispatch entry
/// for it at all.
fn render_minor_version_gate(minor_opcode: u8) -> Option<u32> {
    match minor_opcode {
        // Minor 16 was reserved for a Transform request that never entered
        // the protocol.
        16 => None,
        crate::X_RENDER_QUERY_PICT_INDEX_VALUES_MINOR_OPCODE => Some(7),
        crate::X_RENDER_CREATE_CURSOR_MINOR_OPCODE => Some(5),
        crate::X_RENDER_SET_PICTURE_TRANSFORM_MINOR_OPCODE
        | crate::X_RENDER_QUERY_FILTERS_MINOR_OPCODE
        | crate::X_RENDER_SET_PICTURE_FILTER_MINOR_OPCODE => Some(6),
        crate::X_RENDER_CREATE_ANIM_CURSOR_MINOR_OPCODE => Some(8),
        crate::X_RENDER_ADD_TRAPS_MINOR_OPCODE => Some(9),
        crate::X_RENDER_CREATE_SOLID_FILL_MINOR_OPCODE
        | crate::X_RENDER_CREATE_LINEAR_GRADIENT_MINOR_OPCODE
        | crate::X_RENDER_CREATE_RADIAL_GRADIENT_MINOR_OPCODE
        | crate::X_RENDER_CREATE_CONICAL_GRADIENT_MINOR_OPCODE => Some(10),
        minor if minor <= crate::X_RENDER_LAST_MINOR_OPCODE => Some(0),
        _ => None,
    }
}

fn dispatch_render_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
        XWireRequest::Render(crate::XRenderRequest::RenderQueryVersion { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderQueryPictFormats)
            | XWireRequest::Render(crate::XRenderRequest::RenderQueryFilters { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderUnimplemented { .. })
    ) {
        return Unhandled(request);
    }
    Handled(match request {
        // The answer is the lower of the two versions, and the server's side
        // of that comparison is the constant that moves only when the
        // requests behind the next version answer.
        XWireRequest::Render(crate::XRenderRequest::RenderQueryVersion { major, minor }) => {
            let (major_version, minor_version) = if major == crate::X_RENDER_MAJOR_VERSION {
                (major, minor.min(crate::X_RENDER_MINOR_VERSION))
            } else {
                (crate::X_RENDER_MAJOR_VERSION, crate::X_RENDER_MINOR_VERSION)
            };
            XDispatchResult {
                response: None,
                outputs: vec![XClientOutput::Reply(XClientReply::RenderQueryVersion {
                    sequence: context.sequence,
                    major_version,
                    minor_version,
                })],
                metadata_candidates: Vec::new(),
            }
        }
        // The reply is a constant: the four formats are the four pixel
        // layouts the server can represent, so the encoder owns the table and
        // dispatch carries only the sequence.
        XWireRequest::Render(crate::XRenderRequest::RenderQueryPictFormats) => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Reply(XClientReply::RenderQueryPictFormats {
                sequence: context.sequence,
            })],
            metadata_candidates: Vec::new(),
        },
        XWireRequest::Render(crate::XRenderRequest::RenderQueryFilters { drawable }) => {
            // The drawable selects a screen, and this server has one; it is
            // still validated, because answering for a drawable that does not
            // exist would tell a client its identifier was good.
            let outputs = if runtime
                .validate_drawable_access(context.namespace, drawable)
                .is_err()
            {
                vec![render_error_output(
                    context,
                    XErrorCode::BadDrawable,
                    u32::try_from(drawable.local.raw()).unwrap_or(0),
                    crate::X_RENDER_QUERY_FILTERS_MINOR_OPCODE,
                )]
            } else {
                vec![XClientOutput::Reply(XClientReply::RenderQueryFilters {
                    sequence: context.sequence,
                })]
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        XWireRequest::Render(crate::XRenderRequest::RenderUnimplemented { minor_opcode }) => {
            let code = match render_minor_version_gate(minor_opcode) {
                Some(gate) if gate <= crate::X_RENDER_MINOR_VERSION => {
                    XErrorCode::BadImplementation
                }
                _ => XErrorCode::BadRequest,
            };
            XDispatchResult {
                response: None,
                outputs: vec![XClientOutput::Error(crate::XClientError {
                    code,
                    sequence: context.sequence,
                    resource_id: 0,
                    minor_code: u16::from(minor_opcode),
                    major_code: context.major_opcode,
                })],
                metadata_candidates: Vec::new(),
            }
        }
        other => return Unhandled(other),
    })
}

/// The protocol error each picture-request refusal maps to. The extension
/// has error codes of its own and a client's fallback logic keys on which
/// one arrives, so the mapping is total and explicit.
fn render_picture_error_code(error: crate::XRenderPictureError) -> XErrorCode {
    match error {
        crate::XRenderPictureError::Drawable => XErrorCode::BadDrawable,
        crate::XRenderPictureError::IdInUse => XErrorCode::BadIdChoice,
        crate::XRenderPictureError::UnknownFormat => XErrorCode::RenderPictFormat,
        crate::XRenderPictureError::DepthMismatch => XErrorCode::BadMatch,
        crate::XRenderPictureError::InvalidValue => XErrorCode::BadValue,
        crate::XRenderPictureError::RefusedAttribute => XErrorCode::BadImplementation,
        crate::XRenderPictureError::UnknownPicture => XErrorCode::RenderPicture,
        crate::XRenderPictureError::ParameterMismatch => XErrorCode::BadMatch,
    }
}

/// How an operator without an implementation is refused: the Disjoint,
/// Conjoint and PDF ranges are defined by the protocol and withheld here,
/// while the gaps between them are values no version ever defined -- those
/// get the extension's own PictOp error.
fn render_operator_refusal(op: u8) -> XErrorCode {
    match op {
        0x10..=0x2b | 0x30..=0x3e => XErrorCode::BadImplementation,
        _ => XErrorCode::RenderPictOp,
    }
}

/// The filter a name selects, or `None` for a name this server does not
/// offer.
///
/// Names are matched exactly, as the protocol defines them. The three
/// aliases exist so a client can ask for a quality rather than an algorithm;
/// both `good` and `best` land on bilinear because there is nothing better
/// here to promise.
fn render_filter_from_name(name: &[u8]) -> Option<crate::XRenderPictureFilter> {
    match name {
        b"nearest" | b"fast" => Some(crate::XRenderPictureFilter::Nearest),
        b"bilinear" | b"good" | b"best" => Some(crate::XRenderPictureFilter::Bilinear),
        _ => None,
    }
}

fn render_error_output(
    context: XDispatchContext,
    code: XErrorCode,
    resource_id: u32,
    minor_opcode: u8,
) -> XClientOutput {
    XClientOutput::Error(crate::XClientError {
        code,
        sequence: context.sequence,
        resource_id,
        minor_code: u16::from(minor_opcode),
        major_code: context.major_opcode,
    })
}

fn dispatch_render_picture_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
        XWireRequest::Render(crate::XRenderRequest::RenderCreatePicture { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderChangePicture { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderSetPictureClipRectangles { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderFreePicture { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderFillRectangles { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderComposite { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderSetPictureTransform { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderSetPictureFilter { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderTrapezoids { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderTriangles { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderCreateSolidFill { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderCreateGradient { .. })
    ) {
        return Unhandled(request);
    }
    let lifecycle_result = |outcome: Result<(), crate::XRenderPictureError>,
                            resource_id: u32,
                            minor_opcode: u8| {
        XDispatchResult {
            response: None,
            outputs: outcome
                .err()
                .map(|error| {
                    render_error_output(
                        context,
                        render_picture_error_code(error),
                        resource_id,
                        minor_opcode,
                    )
                })
                .into_iter()
                .collect(),
            metadata_candidates: Vec::new(),
        }
    };
    Handled(match request {
        XWireRequest::Render(crate::XRenderRequest::RenderCreatePicture {
            picture,
            drawable,
            format,
            values,
        }) => lifecycle_result(
            runtime.render_create_picture(
                context.namespace,
                picture,
                drawable,
                format,
                &values,
                u64::from(context.sequence),
            ),
            u32::try_from(picture.local.raw()).unwrap_or(0),
            crate::X_RENDER_CREATE_PICTURE_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderChangePicture { picture, values }) => lifecycle_result(
            runtime.render_change_picture(context.namespace, picture, &values),
            u32::try_from(picture.local.raw()).unwrap_or(0),
            crate::X_RENDER_CHANGE_PICTURE_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderSetPictureClipRectangles {
            picture,
            clip_x_origin,
            clip_y_origin,
            rectangles,
        }) => lifecycle_result(
            runtime.render_set_picture_clip_rectangles(
                context.namespace,
                picture,
                clip_x_origin,
                clip_y_origin,
                rectangles,
            ),
            u32::try_from(picture.local.raw()).unwrap_or(0),
            crate::X_RENDER_SET_PICTURE_CLIP_RECTANGLES_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderFreePicture { picture }) => lifecycle_result(
            runtime.render_free_picture(context.namespace, picture),
            u32::try_from(picture.local.raw()).unwrap_or(0),
            crate::X_RENDER_FREE_PICTURE_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderFillRectangles {
            op,
            picture,
            color,
            rectangles,
        }) => {
            let transaction = context.transaction;
            if !crate::software::render_operator_is_implemented(op) {
                return Handled(XDispatchResult {
                    response: None,
                    outputs: vec![render_error_output(
                        context,
                        render_operator_refusal(op),
                        0,
                        crate::X_RENDER_FILL_RECTANGLES_MINOR_OPCODE,
                    )],
                    metadata_candidates: Vec::new(),
                });
            }
            match runtime.render_apply_fill_rectangles(
                transaction,
                context.namespace,
                op,
                picture,
                color,
                &rectangles,
            ) {
                Ok(response) => {
                    let outputs =
                        if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_RENDER_FILL_RECTANGLES_MINOR_OPCODE),
                                u32::try_from(picture.local.raw()).unwrap_or(0),
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
                Err(error) => XDispatchResult {
                    response: Some(XAuthorityResponsePacket::rejected(
                        transaction,
                        XAuthorityRuntimeError::InvalidResource,
                    )),
                    outputs: vec![render_error_output(
                        context,
                        render_picture_error_code(error),
                        u32::try_from(picture.local.raw()).unwrap_or(0),
                        crate::X_RENDER_FILL_RECTANGLES_MINOR_OPCODE,
                    )],
                    metadata_candidates: Vec::new(),
                },
            }
        }
        XWireRequest::Render(crate::XRenderRequest::RenderComposite {
            op,
            source,
            mask,
            destination,
            source_x,
            source_y,
            mask_x,
            mask_y,
            destination_x,
            destination_y,
            width,
            height,
        }) => {
            let transaction = context.transaction;
            if !crate::software::render_operator_is_implemented(op) {
                return Handled(XDispatchResult {
                    response: None,
                    outputs: vec![render_error_output(
                        context,
                        render_operator_refusal(op),
                        0,
                        crate::X_RENDER_COMPOSITE_MINOR_OPCODE,
                    )],
                    metadata_candidates: Vec::new(),
                });
            }
            match runtime.render_apply_composite(
                transaction,
                context.namespace,
                op,
                source,
                mask,
                destination,
                (source_x, source_y),
                (mask_x, mask_y),
                (destination_x, destination_y),
                width,
                height,
            ) {
                Ok(response) => {
                    let outputs =
                        if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_RENDER_COMPOSITE_MINOR_OPCODE),
                                u32::try_from(destination.local.raw()).unwrap_or(0),
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
                Err(error) => XDispatchResult {
                    response: Some(XAuthorityResponsePacket::rejected(
                        transaction,
                        XAuthorityRuntimeError::InvalidResource,
                    )),
                    outputs: vec![render_error_output(
                        context,
                        render_picture_error_code(error),
                        u32::try_from(destination.local.raw()).unwrap_or(0),
                        crate::X_RENDER_COMPOSITE_MINOR_OPCODE,
                    )],
                    metadata_candidates: Vec::new(),
                },
            }
        }
        XWireRequest::Render(crate::XRenderRequest::RenderSetPictureTransform { picture, matrix }) => lifecycle_result(
            runtime.render_set_picture_transform(context.namespace, picture, matrix),
            u32::try_from(picture.local.raw()).unwrap_or(0),
            crate::X_RENDER_SET_PICTURE_TRANSFORM_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderSetPictureFilter {
            picture,
            name,
            has_params,
        }) => {
            let outcome = match render_filter_from_name(&name) {
                // Only a convolution filter takes parameters, and this server
                // offers none, so a filter it does offer arriving with them is
                // a mismatch between the request and its argument.
                Some(_) if has_params => Err(crate::XRenderPictureError::ParameterMismatch),
                Some(filter) => {
                    runtime.render_set_picture_filter(context.namespace, picture, filter)
                }
                None => Err(crate::XRenderPictureError::InvalidValue),
            };
            lifecycle_result(
                outcome,
                u32::try_from(picture.local.raw()).unwrap_or(0),
                crate::X_RENDER_SET_PICTURE_FILTER_MINOR_OPCODE,
            )
        }
        XWireRequest::Render(crate::XRenderRequest::RenderTrapezoids {
            op,
            source,
            destination,
            mask_format,
            source_x,
            source_y,
            trapezoids,
        }) => render_primitive_result(
            context,
            runtime,
            op,
            source,
            destination,
            mask_format,
            (source_x, source_y),
            crate::XRenderPrimitiveCoverage::Trapezoids(&trapezoids),
            crate::X_RENDER_TRAPEZOIDS_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderTriangles {
            op,
            source,
            destination,
            mask_format,
            source_x,
            source_y,
            triangles,
            minor_opcode,
        }) => render_primitive_result(
            context,
            runtime,
            op,
            source,
            destination,
            mask_format,
            (source_x, source_y),
            crate::XRenderPrimitiveCoverage::Triangles(&triangles),
            minor_opcode,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderCreateSolidFill { picture, color }) => lifecycle_result(
            runtime.render_create_generated_picture(
                context.namespace,
                picture,
                // The wire colour is already premultiplied, so it is stored
                // exactly as it arrived rather than converted.
                crate::XRenderGeneratedSource::Solid([
                    (color[2] >> 8) as u8,
                    (color[1] >> 8) as u8,
                    (color[0] >> 8) as u8,
                    (color[3] >> 8) as u8,
                ]),
                u64::from(context.sequence),
            ),
            u32::try_from(picture.local.raw()).unwrap_or(0),
            crate::X_RENDER_CREATE_SOLID_FILL_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderCreateGradient {
            picture,
            geometry,
            stops,
            minor_opcode,
        }) => {
            // A gradient with no stops has no colour to show anywhere, which
            // the protocol treats as a bad value rather than as an empty
            // picture.
            let outcome = if stops.is_empty() {
                Err(crate::XRenderPictureError::InvalidValue)
            } else {
                runtime.render_create_generated_picture(
                    context.namespace,
                    picture,
                    crate::XRenderGeneratedSource::Gradient { geometry, stops },
                    u64::from(context.sequence),
                )
            };
            lifecycle_result(
                outcome,
                u32::try_from(picture.local.raw()).unwrap_or(0),
                minor_opcode,
            )
        }
        other => return Unhandled(other),
    })
}

fn render_glyph_error_code(error: crate::XRenderGlyphError) -> XErrorCode {
    match error {
        crate::XRenderGlyphError::UnknownGlyphSet => XErrorCode::RenderGlyphSet,
        crate::XRenderGlyphError::UnknownGlyph => XErrorCode::RenderGlyph,
        crate::XRenderGlyphError::IdInUse => XErrorCode::BadIdChoice,
        crate::XRenderGlyphError::UnsupportedFormat => XErrorCode::RenderPictFormat,
        crate::XRenderGlyphError::MalformedGlyphData => XErrorCode::BadLength,
    }
}

fn dispatch_render_glyph_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
        XWireRequest::Render(crate::XRenderRequest::RenderCreateGlyphSet { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderReferenceGlyphSet { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderFreeGlyphSet { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderAddGlyphs { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderFreeGlyphs { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderCompositeGlyphs { .. })
            | XWireRequest::Render(crate::XRenderRequest::RenderCreateCursor { .. })
    ) {
        return Unhandled(request);
    }
    let glyph_result = |outcome: Result<(), crate::XRenderGlyphError>,
                        resource_id: u32,
                        minor_opcode: u8| {
        XDispatchResult {
            response: None,
            outputs: outcome
                .err()
                .map(|error| {
                    render_error_output(
                        context,
                        render_glyph_error_code(error),
                        resource_id,
                        minor_opcode,
                    )
                })
                .into_iter()
                .collect(),
            metadata_candidates: Vec::new(),
        }
    };
    Handled(match request {
        XWireRequest::Render(crate::XRenderRequest::RenderCreateGlyphSet { glyphset, format }) => glyph_result(
            runtime.render_create_glyph_set(
                context.namespace,
                glyphset,
                format,
                u64::from(context.sequence),
            ),
            u32::try_from(glyphset.local.raw()).unwrap_or(0),
            crate::X_RENDER_CREATE_GLYPH_SET_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderReferenceGlyphSet { glyphset, existing }) => glyph_result(
            runtime.render_reference_glyph_set(
                context.namespace,
                glyphset,
                existing,
                u64::from(context.sequence),
            ),
            u32::try_from(glyphset.local.raw()).unwrap_or(0),
            crate::X_RENDER_REFERENCE_GLYPH_SET_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderFreeGlyphSet { glyphset }) => glyph_result(
            runtime.render_free_glyph_set(context.namespace, glyphset),
            u32::try_from(glyphset.local.raw()).unwrap_or(0),
            crate::X_RENDER_FREE_GLYPH_SET_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderAddGlyphs {
            glyphset,
            ids,
            glyphs,
            data,
        }) => glyph_result(
            runtime.render_add_glyphs(context.namespace, glyphset, &ids, &glyphs, &data),
            u32::try_from(glyphset.local.raw()).unwrap_or(0),
            crate::X_RENDER_ADD_GLYPHS_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderFreeGlyphs { glyphset, ids }) => glyph_result(
            runtime.render_free_glyphs(context.namespace, glyphset, &ids),
            u32::try_from(glyphset.local.raw()).unwrap_or(0),
            crate::X_RENDER_FREE_GLYPHS_MINOR_OPCODE,
        ),
        XWireRequest::Render(crate::XRenderRequest::RenderCompositeGlyphs {
            op,
            source,
            destination,
            mask_format: _,
            glyphset,
            source_x,
            source_y,
            elements,
            minor_opcode,
        }) => {
            let transaction = context.transaction;
            if !crate::software::render_operator_is_implemented(op) {
                return Handled(XDispatchResult {
                    response: None,
                    outputs: vec![render_error_output(
                        context,
                        render_operator_refusal(op),
                        0,
                        minor_opcode,
                    )],
                    metadata_candidates: Vec::new(),
                });
            }
            match runtime.render_apply_composite_glyphs(
                transaction,
                context.namespace,
                op,
                source,
                destination,
                glyphset,
                (source_x, source_y),
                &elements,
            ) {
                Ok(response) => {
                    let outputs =
                        if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(minor_opcode),
                                u32::try_from(destination.local.raw()).unwrap_or(0),
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
                Err(error) => {
                    let (code, resource_id) = match error {
                        crate::XRenderCompositeGlyphsError::Picture(error) => (
                            render_picture_error_code(error),
                            u32::try_from(destination.local.raw()).unwrap_or(0),
                        ),
                        crate::XRenderCompositeGlyphsError::Glyph(error) => (
                            render_glyph_error_code(error),
                            u32::try_from(glyphset.local.raw()).unwrap_or(0),
                        ),
                    };
                    XDispatchResult {
                        response: Some(XAuthorityResponsePacket::rejected(
                            transaction,
                            XAuthorityRuntimeError::InvalidResource,
                        )),
                        outputs: vec![render_error_output(
                            context,
                            code,
                            resource_id,
                            minor_opcode,
                        )],
                        metadata_candidates: Vec::new(),
                    }
                }
            }
        }
        XWireRequest::Render(crate::XRenderRequest::RenderCreateCursor {
            cursor,
            source,
            hotspot_x,
            hotspot_y,
        }) => {
            let outcome = runtime.render_create_cursor(
                context.namespace,
                cursor,
                source,
                hotspot_x,
                hotspot_y,
                u64::from(context.sequence),
            );
            XDispatchResult {
                response: None,
                outputs: outcome
                    .err()
                    .map(|error| {
                        let (code, resource_id) = match error {
                            crate::XRenderCursorError::Picture(error) => (
                                render_picture_error_code(error),
                                u32::try_from(source.local.raw()).unwrap_or(0),
                            ),
                            crate::XRenderCursorError::IdInUse => (
                                XErrorCode::BadIdChoice,
                                u32::try_from(cursor.local.raw()).unwrap_or(0),
                            ),
                            // A picture with no alpha cannot describe a
                            // cursor's shape, which is a mismatch between the
                            // argument and the request rather than a bad id.
                            crate::XRenderCursorError::NotArgb32 => (
                                XErrorCode::BadMatch,
                                u32::try_from(source.local.raw()).unwrap_or(0),
                            ),
                            crate::XRenderCursorError::HotspotOutsideImage => {
                                (XErrorCode::BadValue, u32::from(hotspot_x))
                            }
                            // Refused rather than scaled: a cursor silently
                            // resized is a cursor whose hotspot no longer
                            // points where the client put it.
                            crate::XRenderCursorError::TooLarge => (
                                XErrorCode::BadAlloc,
                                u32::try_from(source.local.raw()).unwrap_or(0),
                            ),
                        };
                        render_error_output(
                            context,
                            code,
                            resource_id,
                            crate::X_RENDER_CREATE_CURSOR_MINOR_OPCODE,
                        )
                    })
                    .into_iter()
                    .collect(),
                metadata_candidates: Vec::new(),
            }
        }
        other => return Unhandled(other),
    })
}


/// One coverage composite's outcome as a dispatch result.
///
/// Trapezoids and triangles share every step but the shapes, including the
/// operator check: an operator this server does not implement is refused
/// before any pixel is rasterised.
#[allow(clippy::too_many_arguments)]
fn render_primitive_result(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    op: u8,
    source: crate::XResourceId,
    destination: crate::XResourceId,
    mask_format: u32,
    source_origin: (i16, i16),
    coverage: crate::XRenderPrimitiveCoverage<'_>,
    minor_opcode: u8,
) -> XDispatchResult {
    let transaction = context.transaction;
    if !crate::software::render_operator_is_implemented(op) {
        return XDispatchResult {
            response: None,
            outputs: vec![render_error_output(
                context,
                render_operator_refusal(op),
                0,
                minor_opcode,
            )],
            metadata_candidates: Vec::new(),
        };
    }
    match runtime.render_apply_primitive_coverage(
        transaction,
        context.namespace,
        op,
        source,
        destination,
        mask_format,
        source_origin,
        coverage,
    ) {
        Ok(response) => {
            let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    u16::from(minor_opcode),
                    u32::try_from(destination.local.raw()).unwrap_or(0),
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
        Err(error) => XDispatchResult {
            response: Some(XAuthorityResponsePacket::rejected(
                transaction,
                XAuthorityRuntimeError::InvalidResource,
            )),
            outputs: vec![render_error_output(
                context,
                render_picture_error_code(error),
                u32::try_from(destination.local.raw()).unwrap_or(0),
                minor_opcode,
            )],
            metadata_candidates: Vec::new(),
        },
    }
}
