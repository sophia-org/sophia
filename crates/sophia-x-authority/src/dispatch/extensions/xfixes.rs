fn dispatch_xfixes_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::Xfixes(crate::XFixesRequest::XfixesCreateRegion { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesSetRegion { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesDestroyRegion { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesSelectSelectionInput { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesCombineRegion { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesInvertRegion { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesTranslateRegion { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesRegionExtents { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesFetchRegion { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesCreateRegionFrom { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesExpandRegion { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesUnimplemented { .. })
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::Xfixes(crate::XFixesRequest::XfixesCreateRegion { region, rectangles }) => {
                    let output = runtime
                        .create_xfixes_region(
                            context.namespace,
                            region,
                            rectangles,
                            u64::from(context.sequence),
                        )
                        .err()
                        .map(|error| {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_XFIXES_CREATE_REGION_MINOR_OPCODE),
                                u32::try_from(region.local.raw()).unwrap_or(0)))
                        });
                    XDispatchResult {
                        response: None,
                        outputs: output.into_iter().collect(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Xfixes(crate::XFixesRequest::XfixesSetRegion { region, rectangles }) => {
                    let output = runtime
                        .set_xfixes_region(context.namespace, region, rectangles)
                        .err()
                        .map(|error| {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_XFIXES_SET_REGION_MINOR_OPCODE),
                                u32::try_from(region.local.raw()).unwrap_or(0)))
                        });
                    XDispatchResult {
                        response: None,
                        outputs: output.into_iter().collect(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Xfixes(crate::XFixesRequest::XfixesDestroyRegion { region }) => {
                    let output = runtime
                        .destroy_xfixes_region(context.namespace, region)
                        .err()
                        .map(|error| {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_XFIXES_DESTROY_REGION_MINOR_OPCODE),
                                u32::try_from(region.local.raw()).unwrap_or(0)))
                        });
                    XDispatchResult {
                        response: None,
                        outputs: output.into_iter().collect(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Xfixes(crate::XFixesRequest::XfixesSelectSelectionInput {
                    window,
                    selection,
                    event_mask,
                }) => {
                    let output = if event_mask & !0b111 != 0 {
                        Some(XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: event_mask,
                            minor_code: crate::X_XFIXES_SELECT_SELECTION_INPUT_MINOR_OPCODE.into(),
                            major_code: context.major_opcode,
                        }))
                    } else if atoms.name(selection).is_none() {
                        Some(XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadAtom,
                            sequence: context.sequence,
                            resource_id: selection,
                            minor_code: crate::X_XFIXES_SELECT_SELECTION_INPUT_MINOR_OPCODE.into(),
                            major_code: context.major_opcode,
                        }))
                    } else if let Err(error) =
                        validate_window_or_root_access(runtime, context.namespace, window)
                    {
                        let error = x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_XFIXES_SELECT_SELECTION_INPUT_MINOR_OPCODE),
                            u32::try_from(window.local.raw()).unwrap_or(0));
                        Some(XClientOutput::Error(error))
                    } else {
                        None
                    };
                    XDispatchResult {
                        response: None,
                        outputs: output.into_iter().collect(),
                        metadata_candidates: Vec::new(),
                    }
                }
        XWireRequest::Xfixes(crate::XFixesRequest::XfixesCombineRegion {
            minor_opcode,
            source,
            other,
            destination,
        }) => {
            let combine: fn(&[Rect], &[Rect]) -> Vec<Rect> = match minor_opcode {
                crate::X_XFIXES_UNION_REGION_MINOR_OPCODE => {
                    sophia_protocol::geometry::region_algebra::union
                }
                crate::X_XFIXES_INTERSECT_REGION_MINOR_OPCODE => {
                    sophia_protocol::geometry::region_algebra::intersect
                }
                crate::X_XFIXES_SUBTRACT_REGION_MINOR_OPCODE => {
                    sophia_protocol::geometry::region_algebra::subtract
                }
                // Copy names one source twice, so unioning it with itself is
                // the copy -- and canonicalises on the way, which is what
                // every other operation here leaves behind too.
                _ => sophia_protocol::geometry::region_algebra::union,
            };
            xfixes_region_result(
                context,
                runtime.combine_xfixes_regions(
                    context.namespace,
                    source,
                    other,
                    destination,
                    combine,
                ),
                minor_opcode,
                destination,
            )
        }
        XWireRequest::Xfixes(crate::XFixesRequest::XfixesInvertRegion {
            source,
            bounds,
            destination,
        }) => xfixes_region_result(
            context,
            runtime.invert_xfixes_region(context.namespace, source, bounds, destination),
            crate::X_XFIXES_INVERT_REGION_MINOR_OPCODE,
            destination,
        ),
        XWireRequest::Xfixes(crate::XFixesRequest::XfixesTranslateRegion { region, dx, dy }) => xfixes_region_result(
            context,
            runtime.translate_xfixes_region(context.namespace, region, dx, dy),
            crate::X_XFIXES_TRANSLATE_REGION_MINOR_OPCODE,
            region,
        ),
        XWireRequest::Xfixes(crate::XFixesRequest::XfixesRegionExtents {
            source,
            destination,
        }) => xfixes_region_result(
            context,
            runtime.set_xfixes_region_to_extents(context.namespace, source, destination),
            crate::X_XFIXES_REGION_EXTENTS_MINOR_OPCODE,
            destination,
        ),
        XWireRequest::Xfixes(crate::XFixesRequest::XfixesFetchRegion { region }) => {
            let outputs = match runtime.fetch_xfixes_region(context.namespace, region) {
                Ok(rects) => {
                    let extents = sophia_protocol::geometry::region_algebra::extents(&rects)
                        .unwrap_or_default();
                    vec![XClientOutput::Reply(XClientReply::XfixesFetchRegion {
                        sequence: context.sequence,
                        extents,
                        rects,
                    })]
                }
                Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    u16::from(crate::X_XFIXES_FETCH_REGION_MINOR_OPCODE),
                    u32::try_from(region.local.raw()).unwrap_or(0),
                ))],
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        // Refused by name. This server answers XFIXES 6.0 without every
        // minor behind it, so a minor defined by that version and not
        // implemented says so, and one above it says the version does not
        // reach that far.
        XWireRequest::Xfixes(crate::XFixesRequest::XfixesCreateRegionFrom {
            minor_opcode,
            region,
            source,
            kind,
        }) => {
            let outcome = match minor_opcode {
                crate::X_XFIXES_CREATE_REGION_FROM_BITMAP_MINOR_OPCODE => runtime
                    .create_xfixes_region_from_bitmap(
                        context.namespace,
                        region,
                        source,
                        u64::from(context.sequence),
                    ),
                crate::X_XFIXES_CREATE_REGION_FROM_WINDOW_MINOR_OPCODE => runtime
                    .create_xfixes_region_from_window(
                        context.namespace,
                        region,
                        source,
                        kind,
                        u64::from(context.sequence),
                    ),
                crate::X_XFIXES_CREATE_REGION_FROM_GC_MINOR_OPCODE => runtime
                    .create_xfixes_region_from_gc(
                        context.namespace,
                        region,
                        source,
                        u64::from(context.sequence),
                    ),
                _ => runtime.create_xfixes_region_from_picture(
                    context.namespace,
                    region,
                    source,
                    u64::from(context.sequence),
                ),
            };
            xfixes_source_result(context, outcome, minor_opcode, region, source, kind)
        }
        XWireRequest::Xfixes(crate::XFixesRequest::XfixesExpandRegion {
            source,
            destination,
            left,
            right,
            top,
            bottom,
        }) => xfixes_source_result(
            context,
            runtime.expand_xfixes_region(
                context.namespace,
                source,
                destination,
                left,
                right,
                top,
                bottom,
            ),
            crate::X_XFIXES_EXPAND_REGION_MINOR_OPCODE,
            destination,
            source,
            0,
        ),
        XWireRequest::Xfixes(crate::XFixesRequest::XfixesUnimplemented { minor_opcode }) => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Error(crate::XClientError {
                code: if minor_opcode <= crate::X_XFIXES_LAST_MINOR_OPCODE {
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
        _ => unreachable!("request family checked before dispatch"),
    })
}

/// One region operation's outcome as a dispatch result: nothing on success,
/// a named error on failure.
fn xfixes_region_result(
    context: XDispatchContext,
    outcome: Result<(), XAuthorityRuntimeError>,
    minor_opcode: u8,
    resource: crate::XResourceId,
) -> XDispatchResult {
    XDispatchResult {
        response: None,
        outputs: outcome
            .err()
            .map(|error| {
                XClientOutput::Error(x_error_from_runtime(
                    error,
                    context.sequence,
                    context.major_opcode,
                    u16::from(minor_opcode),
                    u32::try_from(resource.local.raw()).unwrap_or(0),
                ))
            })
            .into_iter()
            .collect(),
        metadata_candidates: Vec::new(),
    }
}

/// One region-source outcome as a dispatch result.
///
/// Each way of failing names the thing the client got wrong, and the error
/// carries that resource rather than the region being created -- a client
/// whose graphics context is gone wants to hear about the graphics context.
#[allow(clippy::too_many_arguments)]
fn xfixes_source_result(
    context: XDispatchContext,
    outcome: Result<(), crate::XFixesSourceError>,
    minor_opcode: u8,
    region: crate::XResourceId,
    source: crate::XResourceId,
    kind: u8,
) -> XDispatchResult {
    let output = outcome.err().map(|error| {
        let (code, resource) = match error {
            crate::XFixesSourceError::IdInUse => (
                XErrorCode::BadIdChoice,
                u32::try_from(region.local.raw()).unwrap_or(0),
            ),
            crate::XFixesSourceError::UnknownPixmap => (
                XErrorCode::BadPixmap,
                u32::try_from(source.local.raw()).unwrap_or(0),
            ),
            // A mask that is not one bit deep cannot describe a region, which
            // is a mismatch between the argument and the request.
            crate::XFixesSourceError::NotABitmap => (
                XErrorCode::BadMatch,
                u32::try_from(source.local.raw()).unwrap_or(0),
            ),
            crate::XFixesSourceError::UnknownWindow => (
                XErrorCode::BadWindow,
                u32::try_from(source.local.raw()).unwrap_or(0),
            ),
            crate::XFixesSourceError::InvalidKind => (XErrorCode::BadValue, u32::from(kind)),
            crate::XFixesSourceError::UnknownGraphicsContext => (
                XErrorCode::BadGraphicsContext,
                u32::try_from(source.local.raw()).unwrap_or(0),
            ),
            // RENDER owns the picture error, and a client branches on it.
            crate::XFixesSourceError::UnknownPicture => (
                XErrorCode::RenderPicture,
                u32::try_from(source.local.raw()).unwrap_or(0),
            ),
            // The source exists and has nothing to copy, which the protocol
            // reports apart from the source not existing at all.
            crate::XFixesSourceError::NoClip => (
                XErrorCode::BadMatch,
                u32::try_from(source.local.raw()).unwrap_or(0),
            ),
            crate::XFixesSourceError::UnknownRegion => (
                XErrorCode::BadWindow,
                u32::try_from(source.local.raw()).unwrap_or(0),
            ),
        };
        XClientOutput::Error(crate::XClientError {
            code,
            sequence: context.sequence,
            resource_id: resource,
            minor_code: u16::from(minor_opcode),
            major_code: context.major_opcode,
        })
    });
    XDispatchResult {
        response: None,
        outputs: output.into_iter().collect(),
        metadata_candidates: Vec::new(),
    }
}
