fn dispatch_present_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    timed: bool,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::Present(crate::XPresentRequest::PresentQueryVersion { .. })
            | XWireRequest::Present(crate::XPresentRequest::PresentQueryCapabilities { .. })
            | XWireRequest::Present(crate::XPresentRequest::PresentSelectInput { .. })
            | XWireRequest::Present(crate::XPresentRequest::PresentNotifyMsc { .. })
            | XWireRequest::Present(crate::XPresentRequest::PresentUnimplemented { .. })
            | XWireRequest::Present(crate::XPresentRequest::PresentPixmap { .. })
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::Present(crate::XPresentRequest::PresentQueryVersion { .. }) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::PresentQueryVersion {
                        sequence: context.sequence,
                        major_version: 1,
                        minor_version: 2,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Present(crate::XPresentRequest::PresentQueryCapabilities { target }) => {
                    // Mesa's DRI3 loader queries capabilities for every drawable it
                    // initialises, offscreen ones included.
                    let outputs = if target.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT)
                        || runtime
                            .validate_dri3_drawable_access(context.namespace, target)
                            .is_ok()
                    {
                        vec![XClientOutput::Reply(
                            XClientReply::PresentQueryCapabilities {
                                sequence: context.sequence,
                                capabilities: 1 << 1,
                            },
                        )]
                    } else {
                        vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadWindow,
                            sequence: context.sequence,
                            resource_id: u32::try_from(target.local.raw()).unwrap_or(0),
                            minor_code: u16::from(crate::X_PRESENT_QUERY_CAPABILITIES_MINOR_OPCODE),
                            major_code: context.major_opcode,
                        })]
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Present(crate::XPresentRequest::PresentSelectInput {
                    window, event_mask, ..
                }) => {
                    let outputs = if event_mask & !0x0f != 0 {
                        vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: event_mask,
                            minor_code: u16::from(crate::X_PRESENT_SELECT_INPUT_MINOR_OPCODE),
                            major_code: context.major_opcode,
                        })]
                    } else if let Err(error) =
                        runtime.validate_present_window(context.namespace, window)
                    {
                        vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_PRESENT_SELECT_INPUT_MINOR_OPCODE),
                            u32::try_from(window.local.raw()).unwrap_or(0),
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
                XWireRequest::Present(crate::XPresentRequest::PresentNotifyMsc { window, serial, target_msc, divisor, remainder }) => {
                    // Void, like SelectInput: the answer is a CompleteNotify of
                    // kind NotifyMSC, delivered by the socket layer from the
                    // presentation clock. Session retains timing here; a
                    // standalone caller without clock service only validates.
                    let outputs = if let Err(error) =
                        runtime.validate_present_window(context.namespace, window)
                    {
                        vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_PRESENT_NOTIFY_MSC_MINOR_OPCODE),
                            u32::try_from(window.local.raw()).unwrap_or(0),
                        ))]
                    } else if present_remainder_is_invalid(divisor, remainder) {
                        vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: remainder as u32,
                            minor_code: u16::from(crate::X_PRESENT_NOTIFY_MSC_MINOR_OPCODE),
                            major_code: context.major_opcode,
                        })]
                    } else if timed {
                        let timing = crate::XPresentMscTiming::notify(target_msc, divisor, remainder)
                            .expect("validated Present modulus");
                        runtime.prepare_present_msc_notify_with_publication(context.client_id, context.transaction,
                            context.namespace, window, serial, timing, crate::runtime::XPresentPublication::PendingWire)
                            .err().map(|error| XClientOutput::Error(present_preparation_error(
                                context, error, window, crate::X_PRESENT_NOTIFY_MSC_MINOR_OPCODE)))
                            .into_iter().collect()
                    } else {
                        Vec::new()
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                // Decoded, and refused where the client can see it.
                XWireRequest::Present(crate::XPresentRequest::PresentUnimplemented { minor_opcode }) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Error(crate::XClientError {
                        code: if minor_opcode <= crate::X_PRESENT_LAST_MINOR_OPCODE {
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
                XWireRequest::Present(crate::XPresentRequest::PresentPixmap {
                    transaction,
                    window,
                    pixmap,
                    valid_region,
                    update_region,
                    target_crtc,
                    wait_fence,
                    idle_fence,
                    x_offset,
                    y_offset,
                    options,
                    target_msc,
                    divisor,
                    remainder,
                    ..
                }) => {
                    let value_error = |value| crate::XClientError {
                        code: XErrorCode::BadValue,
                        sequence: context.sequence,
                        resource_id: value,
                        minor_code: u16::from(crate::X_PRESENT_PIXMAP_MINOR_OPCODE),
                        major_code: context.major_opcode,
                    };
                    let resource_error = |error, id: XResourceId, missing| {
                        let mut error = x_error_from_runtime(error, context.sequence,
                            context.major_opcode, u16::from(crate::X_PRESENT_PIXMAP_MINOR_OPCODE),
                            id.local.raw() as u32);
                        if error.code == XErrorCode::BadWindow { error.code = missing; }
                        error
                    };
                    // Match request validation order: window, pixmap, regions,
                    // CRTC, fences, options, remainder. A bad scalar must not
                    // conceal an earlier invalid resource.
                    let validation = (|| {
                        runtime.validate_present_window(context.namespace, window)
                            .map_err(|e| resource_error(e, window, XErrorCode::BadWindow))?;
                        runtime.validate_pixmap_access(context.namespace, pixmap)
                            .map_err(|e| resource_error(e, pixmap, XErrorCode::BadPixmap))?;
                        for region in [valid_region, update_region] {
                            if region != 0 {
                                let id = XResourceId::new(u64::from(region), 1);
                                runtime.validate_xfixes_region_access(context.namespace, id)
                                    .map_err(|e| resource_error(e, id, XErrorCode::BadValue))?;
                            }
                        }
                        // Explicit CRTC selection remains unsupported.
                        if target_crtc != 0 { return Err(value_error(target_crtc)); }
                        for fence in [wait_fence, idle_fence].into_iter().flatten() {
                            runtime.validate_dri3_fence_access(context.namespace, fence)
                                .map_err(|e| resource_error(e, fence, XErrorCode::BadValue))?;
                        }
                        // Version 1.2 has no AsyncMayTear. UST conversion is
                        // required even without CapabilityUST; until supplied,
                        // refuse it rather than interpreting microseconds as MSC.
                        if options & !0x0f != 0 || options & (1 << 2) != 0 {
                            return Err(value_error(options));
                        }
                        if present_remainder_is_invalid(divisor, remainder) {
                            return Err(value_error(remainder as u32));
                        }
                        Ok(())
                    })();
                    if let Err(error) = validation {
                        return Handled(XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(error)],
                            metadata_candidates: Vec::new(),
                        });
                    }
                    let valid_region = (valid_region != 0)
                        .then(|| {
                            runtime.xfixes_region_snapshot(
                                context.namespace,
                                XResourceId::new(u64::from(valid_region), 1),
                            )
                        })
                        .transpose()
                        .expect("validated Present valid region must remain available");
                    let update_region = (update_region != 0)
                        .then(|| {
                            runtime.xfixes_region_snapshot(
                                context.namespace,
                                XResourceId::new(u64::from(update_region), 1),
                            )
                        })
                        .transpose()
                        .expect("validated Present update region must remain available");
                    if timed {
                        let prepared = runtime.prepare_standard_pixmap_with_publication(context.client_id, transaction,
                            context.namespace, window, pixmap, (x_offset, y_offset), (valid_region,
                            update_region), crate::XPresentFenceResources { wait: wait_fence, idle: idle_fence },
                            crate::runtime::XPresentPublication::PendingWire);
                        let outputs = if let Err(error) = prepared {
                            vec![XClientOutput::Error(present_preparation_error(
                                context, error, window, crate::X_PRESENT_PIXMAP_MINOR_OPCODE))]
                        } else {
                            let timing = crate::XPresentMscTiming::new(target_msc, divisor, remainder, options & 1 != 0)
                                .expect("validated Present modulus");
                            // This fresh, unscheduled preparation has a unique ticket.
                            if let Err(error) = runtime.request_prepared_present_clock(transaction, timing) {
                                runtime.cancel_prepared_standard_pixmap(transaction);
                                debug_assert!(false, "fresh Present timing admission failed: {error:?}");
                                vec![XClientOutput::Error(crate::XClientError {
                                    code: XErrorCode::BadAlloc, ..value_error(window.local.raw() as u32)
                                })]
                            } else { Vec::new() }
                        };
                        return Handled(XDispatchResult { response: None, outputs, metadata_candidates: Vec::new() });
                    }
                    let response = runtime.present_standard_pixmap(
                        transaction,
                        context.namespace,
                        window,
                        pixmap,
                        x_offset,
                        y_offset,
                        valid_region,
                        update_region,
                    );
                    let outputs = match response.outcome {
                        XAuthorityResponseOutcome::Accepted => Vec::new(),
                        XAuthorityResponseOutcome::Rejected(error) => {
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_PRESENT_PIXMAP_MINOR_OPCODE),
                                u32::try_from(pixmap.local.raw()).unwrap_or(0),
                            ))]
                        }
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

fn present_remainder_is_invalid(divisor: u64, remainder: u64) -> bool {
    if divisor == 0 { remainder != 0 } else { remainder >= divisor }
}

fn present_preparation_error(context: XDispatchContext, error: crate::XPresentPreparationError,
    window: XResourceId, minor: u8) -> crate::XClientError
{
    if let crate::XPresentPreparationError::Invalid(error) = error {
        return x_error_from_runtime(error, context.sequence, context.major_opcode,
            u16::from(minor), window.local.raw() as u32);
    }
    debug_assert_ne!(error, crate::XPresentPreparationError::DuplicateTransaction,
        "fresh Present request reused an authority ticket");
    crate::XClientError {
        code: XErrorCode::BadAlloc, sequence: context.sequence,
        resource_id: window.local.raw() as u32, minor_code: u16::from(minor),
        major_code: context.major_opcode,
    }
}
