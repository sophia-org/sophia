fn dispatch_dri3_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::Dri3(crate::XDri3Request::Dri3Open { .. })
            | XWireRequest::Dri3(crate::XDri3Request::Dri3PixmapFromBuffer { .. })
            | XWireRequest::Dri3(crate::XDri3Request::Dri3PixmapFromBuffers { .. })
            | XWireRequest::Dri3(crate::XDri3Request::Dri3FenceFromFd { .. })
            | XWireRequest::Dri3(crate::XDri3Request::Dri3GetSupportedModifiers { .. })
            | XWireRequest::Dri3(crate::XDri3Request::Dri3SetDrmDeviceInUse { .. })
            | XWireRequest::Dri3(crate::XDri3Request::Dri3BufferFromPixmap { .. })
            | XWireRequest::Dri3(crate::XDri3Request::Dri3BuffersFromPixmap { .. })
            | XWireRequest::Dri3(crate::XDri3Request::Dri3Unimplemented { .. })
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::Dri3(crate::XDri3Request::Dri3Open { drawable, provider }) => {
                    let outputs = if provider != 0 {
                        vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: provider,
                            minor_code: u16::from(crate::X_DRI3_OPEN_MINOR_OPCODE),
                            major_code: context.major_opcode,
                        })]
                    } else if let Err(error) = runtime.validate_dri3_drawable_access(context.namespace, drawable)
                    {
                        let error = x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_DRI3_OPEN_MINOR_OPCODE),
                            u32::try_from(drawable.local.raw()).unwrap_or(0));
                        vec![XClientOutput::Error(error)]
                    } else {
                        vec![XClientOutput::Reply(XClientReply::Dri3Open {
                            sequence: context.sequence,
                        })]
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Dri3(crate::XDri3Request::Dri3PixmapFromBuffer {
                    pixmap,
                    drawable,
                    size_bytes,
                    width,
                    height,
                    stride,
                    depth,
                    bits_per_pixel,
                }) => {
                    let outputs =
                        if let Err(error) = runtime.validate_dri3_drawable_access(context.namespace, drawable) {
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_DRI3_PIXMAP_FROM_BUFFER_MINOR_OPCODE),
                                u32::try_from(drawable.local.raw()).unwrap_or(0)))]
                        } else if let Err(error) = runtime.create_dri3_pixmap(
                            context.namespace,
                            pixmap,
                            u64::from(context.sequence),
                            size_bytes,
                            width,
                            height,
                            stride,
                            depth,
                            bits_per_pixel,
                        ) {
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_DRI3_PIXMAP_FROM_BUFFER_MINOR_OPCODE),
                                u32::try_from(pixmap.local.raw()).unwrap_or(0)))]
                        } else {
                            Vec::new()
                        };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Dri3(crate::XDri3Request::Dri3PixmapFromBuffers {
                    pixmap,
                    window,
                    num_buffers,
                    width,
                    height,
                    strides,
                    offsets,
                    depth,
                    bits_per_pixel,
                    modifier,
                }) => {
                    let outputs =
                        if let Err(error) = runtime.validate_dri3_drawable_access(context.namespace, window) {
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_DRI3_PIXMAP_FROM_BUFFERS_MINOR_OPCODE),
                                u32::try_from(window.local.raw()).unwrap_or(0)))]
                        } else if let Err(error) = runtime.create_dri3_pixmap_from_buffers(
                            context.namespace,
                            pixmap,
                            u64::from(context.sequence),
                            num_buffers,
                            width,
                            height,
                            strides,
                            offsets,
                            depth,
                            bits_per_pixel,
                            modifier,
                        ) {
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_DRI3_PIXMAP_FROM_BUFFERS_MINOR_OPCODE),
                                u32::try_from(pixmap.local.raw()).unwrap_or(0)))]
                        } else {
                            Vec::new()
                        };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Dri3(crate::XDri3Request::Dri3FenceFromFd {
                    drawable, fence, ..
                }) => {
                    let outputs =
                        if let Err(error) = runtime.validate_dri3_drawable_access(context.namespace, drawable) {
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_DRI3_FENCE_FROM_FD_MINOR_OPCODE),
                                u32::try_from(drawable.local.raw()).unwrap_or(0)))]
                        } else if let Err(error) =
                            runtime.create_dri3_fence(context.namespace, fence, u64::from(context.sequence))
                        {
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_DRI3_FENCE_FROM_FD_MINOR_OPCODE),
                                u32::try_from(fence.local.raw()).unwrap_or(0)))]
                        } else {
                            Vec::new()
                        };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Dri3(crate::XDri3Request::Dri3BuffersFromPixmap { pixmap }) => {
                    let outputs = match runtime
                        .dri3_pixmap_buffers(context.namespace, pixmap)
                    {
                        Ok((descriptor, _)) => {
                            let planes = usize::from(descriptor.plane_count);
                            vec![XClientOutput::Reply(XClientReply::Dri3BuffersFromPixmap {
                                sequence: context.sequence,
                                width: u16::try_from(descriptor.size.width).unwrap_or(0),
                                height: u16::try_from(descriptor.size.height).unwrap_or(0),
                                modifier: descriptor.modifier,
                                depth: crate::dri3_depth_of(descriptor.format),
                                bits_per_pixel: 32,
                                strides: descriptor
                                    .planes
                                    .iter()
                                    .take(planes)
                                    .map(|plane| plane.map_or(0, |plane| plane.stride))
                                    .collect(),
                                offsets: descriptor
                                    .planes
                                    .iter()
                                    .take(planes)
                                    .map(|plane| plane.map_or(0, |plane| plane.offset))
                                    .collect(),
                            })]
                        }
                        // These requests name a PIXMAP. A drawable that is
                        // not one -- or one carrying no imported buffer -- is a
                        // pixmap fault, not a window fault: reporting BadWindow
                        // would send the client looking at the wrong resource.
                        Err(_) => vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadPixmap,
                            sequence: context.sequence,
                            resource_id: u32::try_from(pixmap.local.raw()).unwrap_or(0),
                            minor_code: u16::from(crate::X_DRI3_BUFFERS_FROM_PIXMAP_MINOR_OPCODE),
                            major_code: context.major_opcode,
                        })]
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Dri3(crate::XDri3Request::Dri3BufferFromPixmap { pixmap }) => {
                    let outputs = match runtime
                        .dri3_pixmap_buffers(context.namespace, pixmap)
                    {
                        // Legacy replies carry no offset and only a 16-bit stride.
                        Ok((descriptor, _)) if descriptor.plane_count == 1
                            && descriptor.planes[0].is_some_and(|plane|
                                plane.offset == 0 && u16::try_from(plane.stride).is_ok()) => {
                            let stride = descriptor.planes[0].map_or(0, |plane| plane.stride);
                            let height = u16::try_from(descriptor.size.height).unwrap_or(0);
                            vec![XClientOutput::Reply(XClientReply::Dri3BufferFromPixmap {
                                sequence: context.sequence,
                                size_bytes: stride * u32::from(height),
                                width: u16::try_from(descriptor.size.width).unwrap_or(0),
                                height,
                                stride: u16::try_from(stride).expect("legacy stride was checked"),
                                depth: crate::dri3_depth_of(descriptor.format),
                                bits_per_pixel: 32,
                            })]
                        }
                        Ok((descriptor, _)) if descriptor.plane_count == 1 =>
                            vec![XClientOutput::Error(crate::XClientError {
                                code: XErrorCode::BadPixmap,
                                sequence: context.sequence,
                                resource_id: u32::try_from(pixmap.local.raw()).unwrap_or(0),
                                minor_code: u16::from(crate::X_DRI3_BUFFER_FROM_PIXMAP_MINOR_OPCODE),
                                major_code: context.major_opcode,
                            })],
                        Ok(_) => vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadMatch,
                            sequence: context.sequence,
                            resource_id: u32::try_from(pixmap.local.raw()).unwrap_or(0),
                            minor_code: u16::from(crate::X_DRI3_BUFFER_FROM_PIXMAP_MINOR_OPCODE),
                            major_code: context.major_opcode,
                        })],
                        // These requests name a PIXMAP. A drawable that is
                        // not one -- or one carrying no imported buffer -- is a
                        // pixmap fault, not a window fault: reporting BadWindow
                        // would send the client looking at the wrong resource.
                        Err(_) => vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadPixmap,
                            sequence: context.sequence,
                            resource_id: u32::try_from(pixmap.local.raw()).unwrap_or(0),
                            minor_code: u16::from(crate::X_DRI3_BUFFER_FROM_PIXMAP_MINOR_OPCODE),
                            major_code: context.major_opcode,
                        })]
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                // Decoded, and refused where the client can see it. Sophia
                // advertises the DRI3 version whose requests it answers; the
                // ones it does not answer owe a normal error rather than a
                // dropped connection.
                XWireRequest::Dri3(crate::XDri3Request::Dri3Unimplemented { minor_opcode }) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Error(crate::XClientError {
                        code: if minor_opcode <= crate::X_DRI3_LAST_MINOR_OPCODE {
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
                XWireRequest::Dri3(crate::XDri3Request::Dri3SetDrmDeviceInUse { window, major, minor }) => {
                    let outputs = match runtime.set_window_render_device_hint(
                        context.namespace, window, crate::XDrmDeviceHint { major, minor },
                    ) {
                        Ok(()) => Vec::new(),
                        Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                            error, context.sequence, context.major_opcode,
                            u16::from(crate::X_DRI3_SET_DRM_DEVICE_IN_USE_MINOR_OPCODE),
                            u32::try_from(window.local.raw()).unwrap_or(0),
                        ))],
                    };
                    XDispatchResult { response: None, outputs, metadata_candidates: Vec::new() }
                }
                XWireRequest::Dri3(crate::XDri3Request::Dri3GetSupportedModifiers {
                    window,
                    depth,
                    bits_per_pixel,
                }) => {
                    let screen_modifiers = match (depth, bits_per_pixel) {
                        (24, 32) => runtime.dma_buf_import_modifiers_for_client(
                            context.client_id, sophia_protocol::DRM_FORMAT_XRGB8888).to_vec(),
                        (32, 32) => runtime.dma_buf_import_modifiers_for_client(
                            context.client_id, sophia_protocol::DRM_FORMAT_ARGB8888).to_vec(),
                        _ => Vec::new(),
                    };
                    let format = if depth == 32 { sophia_protocol::DRM_FORMAT_ARGB8888 }
                        else { sophia_protocol::DRM_FORMAT_XRGB8888 };
                    let window_modifiers = runtime.window_allocation_modifiers(
                        context.namespace, context.client_id, window, format,
                    );
                    let outputs = if !matches!((depth, bits_per_pixel), (24 | 32, 32)) {
                        vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: u32::from(depth),
                            minor_code: u16::from(crate::X_DRI3_GET_SUPPORTED_MODIFIERS_MINOR_OPCODE),
                            major_code: context.major_opcode,
                        })]
                    } else if window.local.raw() != u64::from(crate::X_SETUP_DEFAULT_ROOT) {
                        match runtime.validate_dri3_drawable_access(context.namespace, window) {
                            Ok(()) => vec![XClientOutput::Reply(
                                XClientReply::Dri3GetSupportedModifiers {
                                    sequence: context.sequence,
                                    window_modifiers,
                                    screen_modifiers,
                                },
                            )],
                            Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_DRI3_GET_SUPPORTED_MODIFIERS_MINOR_OPCODE),
                                u32::try_from(window.local.raw()).unwrap_or(0)))],
                        }
                    } else {
                        vec![XClientOutput::Reply(
                            XClientReply::Dri3GetSupportedModifiers {
                                sequence: context.sequence,
                                window_modifiers,
                                screen_modifiers,
                            },
                        )]
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
