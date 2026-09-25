fn dispatch_shm_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::Extension(crate::XExtensionRequest::BigRequestsEnable)
            | XWireRequest::Shm(crate::XShmRequest::ShmAttach { .. })
            | XWireRequest::Shm(crate::XShmRequest::ShmAttachFd { .. })
            | XWireRequest::Shm(crate::XShmRequest::ShmCreateSegment { .. })
            | XWireRequest::Shm(crate::XShmRequest::ShmDetach { .. })
            | XWireRequest::Shm(crate::XShmRequest::ShmCreatePixmap { .. })
            | XWireRequest::Shm(crate::XShmRequest::ShmPutImage { .. })
            | XWireRequest::Shm(crate::XShmRequest::ShmGetImage { .. })
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::Extension(crate::XExtensionRequest::BigRequestsEnable) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::BigRequestsEnable {
                        sequence: context.sequence,
                        maximum_request_length: u32::from(crate::X_SETUP_DEFAULT_MAX_REQUEST_UNITS),
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Shm(crate::XShmRequest::ShmAttach {
                    segment,
                    shmid,
                    read_only,
                }) => {
                    let outputs = match runtime.attach_shm_segment(
                        context.namespace,
                        segment,
                        shmid,
                        read_only,
                        u64::from(context.sequence),
                    ) {
                        Ok(()) => Vec::new(),
                        Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_MIT_SHM_ATTACH_MINOR_OPCODE),
                            u32::try_from(segment.local.raw()).unwrap_or(0)))],
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                // The descriptor is not here: the socket layer owns it and
                // records the segment once it has mapped it. This validates
                // the name so a client learns about a bad id from the request
                // that used it, and nothing is recorded that has no memory.
                XWireRequest::Shm(crate::XShmRequest::ShmAttachFd { segment, .. }) => XDispatchResult {
                    response: None,
                    outputs: if runtime
                        .validate_shm_segment_access(context.namespace, segment)
                        .is_ok()
                    {
                        vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadIdChoice,
                            sequence: context.sequence,
                            resource_id: u32::try_from(segment.local.raw()).unwrap_or(0),
                            minor_code: u16::from(crate::X_MIT_SHM_ATTACH_FD_MINOR_OPCODE),
                            major_code: context.major_opcode,
                        })]
                    } else {
                        Vec::new()
                    },
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Shm(crate::XShmRequest::ShmCreateSegment {
                    segment,
                    size,
                    read_only,
                }) => {
                    let outputs = if runtime
                        .validate_shm_segment_access(context.namespace, segment)
                        .is_ok()
                    {
                        vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadIdChoice,
                            sequence: context.sequence,
                            resource_id: u32::try_from(segment.local.raw()).unwrap_or(0),
                            minor_code: u16::from(crate::X_MIT_SHM_CREATE_SEGMENT_MINOR_OPCODE),
                            major_code: context.major_opcode,
                        })]
                    } else {
                        match runtime.create_shm_descriptor_segment(
                            context.namespace,
                            segment,
                            size,
                            read_only,
                            u64::from(context.sequence),
                        ) {
                            // The descriptor rides out with the reply, put
                            // there by the socket layer.
                            Ok(()) => vec![XClientOutput::Reply(XClientReply::ShmCreateSegment {
                                sequence: context.sequence,
                            })],
                            // A size beyond what this will map is the ordinary
                            // way here: the request carries a CARD32 and the
                            // adapter has a ceiling.
                            Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_MIT_SHM_CREATE_SEGMENT_MINOR_OPCODE),
                                u32::try_from(segment.local.raw()).unwrap_or(0),
                            ))],
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Shm(crate::XShmRequest::ShmDetach { segment }) => {
                    let outputs = match runtime.detach_shm_segment(context.namespace, segment) {
                        Ok(()) => Vec::new(),
                        Err(
                            XAuthorityRuntimeError::InvalidResource
                            | XAuthorityRuntimeError::UnknownResource,
                        ) => Vec::new(),
                        Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_MIT_SHM_DETACH_MINOR_OPCODE),
                            u32::try_from(segment.local.raw()).unwrap_or(0)))],
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Shm(crate::XShmRequest::ShmCreatePixmap {
                    pixmap,
                    drawable,
                    width,
                    height,
                    depth,
                    segment,
                    offset,
                }) => {
                    let valid_shape = width != 0
                        && height != 0
                        && matches!(depth, 24 | 32)
                        && usize::from(width)
                            .checked_mul(usize::from(height))
                            .and_then(|pixels| pixels.checked_mul(4))
                            .and_then(|bytes| usize::try_from(offset).ok()?.checked_add(bytes))
                            .is_some_and(|end| end <= 64 * 1024 * 1024);
                    let result = runtime
                        .validate_drawable_access(context.namespace, drawable)
                        .and_then(|()| runtime.validate_shm_segment_access(context.namespace, segment))
                        .and_then(|()| {
                            valid_shape
                                .then_some(())
                                .ok_or(crate::XAuthorityRuntimeError::InvalidResource)
                        })
                        .and_then(|()| {
                            runtime.create_shm_pixmap(
                                context.namespace,
                                pixmap,
                                sophia_protocol::Size {
                                    width: i32::from(width),
                                    height: i32::from(height),
                                },
                                depth,
                                u64::from(context.sequence),
                                segment,
                                offset,
                            )
                        });
                    let outputs = result
                        .err()
                        .map(|error| {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_MIT_SHM_CREATE_PIXMAP_MINOR_OPCODE),
                                u32::try_from(pixmap.local.raw()).unwrap_or(0)))
                        })
                        .into_iter()
                        .collect();
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Shm(crate::XShmRequest::ShmPutImage {
                    drawable,
                    gc,
                    segment,
                    total_width,
                    total_height,
                    src_x,
                    src_y,
                    src_width,
                    src_height,
                    dst_x,
                    dst_y,
                    depth,
                    format,
                    offset,
                    send_event,
                    ..
                }) => {
                    let transaction = context.transaction;
                    if runtime
                        .validate_shm_segment_access(context.namespace, segment)
                        .is_err()
                    {
                        return Handled(XDispatchResult {
                            response: Some(XAuthorityResponsePacket::accepted(transaction)),
                            outputs: vec![XClientOutput::Error(crate::XClientError {
                                code: XErrorCode::BadAccess,
                                sequence: context.sequence,
                                resource_id: u32::try_from(segment.local.raw()).unwrap_or(0),
                                minor_code: 3,
                                major_code: context.major_opcode,
                            })],
                            metadata_candidates: Vec::new(),
                        });
                    }
                    let damage = Region::single(Rect {
                        x: i32::from(dst_x),
                        y: i32::from(dst_y),
                        width: i32::from(src_width),
                        height: i32::from(src_height),
                    });
                    let image = runtime
                        .shm_segment_mapping(context.namespace, segment)
                        .ok()
                        .and_then(|mapping| {
                            copy_shm_image_region(
                                XShmImageCopy {
                                byte_order: context.byte_order,
                                offset,
                                total_width,
                                total_height,
                                src_x,
                                src_y,
                                src_width,
                                src_height,
                                depth,
                                format,
                                },
                                |offset, len| mapping.copy_bytes(offset, len).ok(),
                            )
                        });
                    // `copy_shm_image_region` already normalized the segment
                    // into tight canonical pixel rows or returned nothing,
                    // so the journal sees the normalized form rather than the
                    // client's original format and padding.
                    let semantics = image.as_ref().and_then(|_| {
                        runtime
                            .graphics_context_values(context.namespace, gc)
                            .ok()
                            .map(|values| XPutImageSemantics {
                                format: X_IMAGE_FORMAT_Z_PIXMAP,
                                depth,
                                left_pad: 0,
                                byte_order: context.byte_order,
                                gc: values,
                            })
                    });
                    let response = runtime.draw_through_inferiors(transaction, context.namespace, drawable, semantics.as_ref().map_or(crate::X_CLIP_BY_CHILDREN, |semantics| semantics.gc.subwindow_mode), |runtime| runtime.apply_put_image(
                        transaction,
                        context.namespace,
                        drawable,
                        damage,
                        image.as_deref(),
                        semantics.as_ref(),
                    ));
                    let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
                        tracing::debug!(?error, depth, format, total_width, total_height, src_x, src_y, src_width, src_height, offset, image_copied=image.is_some(), gc_valid=semantics.is_some(), "MIT-SHM upload rejected");
                        vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_MIT_SHM_PUT_IMAGE_MINOR_OPCODE),
                            u32::try_from(drawable.local.raw()).unwrap_or(0)))]
                    } else if send_event {
                        vec![XClientOutput::Event(XClientEvent::ShmCompletion {
                            sequence: context.sequence,
                            drawable,
                            segment,
                            offset,
                        })]
                    } else {
                        Vec::new()
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Shm(crate::XShmRequest::ShmGetImage {
                    drawable,
                    x,
                    y,
                    width,
                    height,
                    format,
                    plane_mask,
                    segment,
                    offset,
                }) => {
                    let region = Rect {
                        x: i32::from(x),
                        y: i32::from(y),
                        width: i32::from(width),
                        height: i32::from(height),
                    };
                    let result = runtime
                        .validate_shm_segment_access(context.namespace, segment)
                        .map_err(XShmGetImageError::Runtime)
                        .and_then(|()| {
                            crate::image::read_drawable_image(
                                runtime,
                                context.namespace,
                                drawable,
                                region,
                                format,
                                plane_mask,
                                context.byte_order,
                            )
                            .map_err(XShmGetImageError::Image)
                        })
                        .and_then(|readback| {
                            // A segment the client attached read-only is not
                            // somewhere pixels may be written back to, and
                            // saying so is the whole meaning of the flag.
                            if runtime
                                .shm_segment_is_read_only(context.namespace, segment)
                                .map_err(XShmGetImageError::Runtime)?
                            {
                                return Err(XShmGetImageError::Runtime(
                                    XAuthorityRuntimeError::CrossNamespaceDenied,
                                ));
                            }
                            let mapping = runtime
                                .shm_segment_mapping(context.namespace, segment)
                                .map_err(XShmGetImageError::Runtime)?;
                            mapping
                                .write_bytes(
                                    usize::try_from(offset).map_err(|_| {
                                        XShmGetImageError::Runtime(
                                            XAuthorityRuntimeError::InvalidResource,
                                        )
                                    })?,
                                    &readback.data,
                                )
                                .map_err(|_| {
                                    XShmGetImageError::Runtime(
                                        XAuthorityRuntimeError::InvalidResource,
                                    )
                                })?;
                            Ok(readback)
                        });
                    let outputs = match result {
                        Ok(readback) => vec![XClientOutput::Reply(XClientReply::ShmGetImage {
                            sequence: context.sequence,
                            depth: readback.depth,
                            visual: readback.visual,
                            size: u32::try_from(readback.data.len()).unwrap_or(u32::MAX),
                        })],
                        Err(XShmGetImageError::Image(error)) => {
                            vec![XClientOutput::Error(crate::image::image_client_error(
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_MIT_SHM_GET_IMAGE_MINOR_OPCODE),
                                drawable,
                                format,
                                error,
                            ))]
                        }
                        Err(XShmGetImageError::Runtime(error)) => {
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_MIT_SHM_GET_IMAGE_MINOR_OPCODE),
                                u32::try_from(segment.local.raw()).unwrap_or(0)))]
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
        _ => unreachable!("request family checked before dispatch"),
    })
}

enum XShmGetImageError {
    Runtime(XAuthorityRuntimeError),
    Image(crate::image::XImageReadbackError),
}
