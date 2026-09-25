fn dispatch_core_grab_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
    _properties: &mut XPropertyTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::Core(crate::XCoreRequest::GrabPointer { .. })
            | XWireRequest::Core(crate::XCoreRequest::UngrabPointer { .. })
            | XWireRequest::Core(crate::XCoreRequest::ChangeActivePointerGrab { .. })
            | XWireRequest::Core(crate::XCoreRequest::GrabKeyboard { .. })
            | XWireRequest::Core(crate::XCoreRequest::UngrabKeyboard { .. })
            | XWireRequest::Core(crate::XCoreRequest::GrabButton { .. })
            | XWireRequest::Core(crate::XCoreRequest::UngrabButton { .. })
            | XWireRequest::Core(crate::XCoreRequest::GrabKey { .. })
            | XWireRequest::Core(crate::XCoreRequest::UngrabKey { .. })
            | XWireRequest::Core(crate::XCoreRequest::AllowEvents { .. })
            | XWireRequest::Core(crate::XCoreRequest::GrabServer)
            | XWireRequest::Core(crate::XCoreRequest::UngrabServer)
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::Core(crate::XCoreRequest::GrabPointer {
                    window,
                    event_mask,
                    owner_events,
                    pointer_mode,
                    keyboard_mode,
                    ..
                }) => {
                    let status = if validate_window_or_root_access(runtime, context.namespace, window).is_err() {
                        3
                    } else {
                        runtime
                            .input_authority_mut()
                            .grab_pointer(
                                context.namespace,
                                crate::XActiveInputGrab {
                                    owner: context.client_id,
                                    window,
                                    owner_events,
                                    pointer_mode,
                                    keyboard_mode,
                                    event_mask,
                                    xi_event_mask: [0; 8],
                                    xi_event_mask_words: 0,
                                    route_lease: None,
                                },
                            )
                            .map_or(1, |_| 0)
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![XClientOutput::Reply(XClientReply::GrabStatus {
                            sequence: context.sequence,
                            status,
                        })],
                        metadata_candidates: Vec::new(),
                    }
                }
                // The event mask of an active grab this client holds; without
                // one the request has no effect, as the protocol says. The
                // cursor is validated and not applied: cursor display is
                // config-driven here, the same debt WarpPointer carries.
                XWireRequest::Core(crate::XCoreRequest::ChangeActivePointerGrab {
                    cursor,
                    event_mask,
                    ..
                }) => {
                    let outputs = if cursor.local.raw() != 0
                        && runtime
                            .validate_cursor_access(context.namespace, cursor)
                            .is_err()
                    {
                        vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadCursor,
                            sequence: context.sequence,
                            resource_id: u32::try_from(cursor.local.raw()).unwrap_or(0),
                            minor_code: 0,
                            major_code: context.major_opcode,
                        })]
                    } else {
                        runtime.input_authority_mut().change_active_pointer_grab(
                            context.namespace,
                            context.client_id,
                            event_mask,
                        );
                        Vec::new()
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::UngrabPointer { .. }) => {
                    runtime
                        .input_authority_mut()
                        .ungrab_pointer(context.namespace, context.client_id);
                    XDispatchResult {
                        response: None,
                        outputs: Vec::new(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::GrabKeyboard {
                    window,
                    owner_events,
                    pointer_mode,
                    keyboard_mode,
                    ..
                }) => {
                    let status = if validate_window_or_root_access(runtime, context.namespace, window).is_err() {
                        3
                    } else {
                        runtime
                            .input_authority_mut()
                            .grab_keyboard(
                                context.namespace,
                                crate::XActiveInputGrab {
                                    owner: context.client_id,
                                    window,
                                    owner_events,
                                    pointer_mode,
                                    keyboard_mode,
                                    // Core keyboard grabs select both key
                                    // transitions; the protocol has no mask
                                    // parameter for the caller to supply.
                                    event_mask: 3,
                                    xi_event_mask: [0; 8],
                                    xi_event_mask_words: 0,
                                    route_lease: None,
                                },
                            )
                            .map_or(1, |_| 0)
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![XClientOutput::Reply(XClientReply::GrabStatus {
                            sequence: context.sequence,
                            status,
                        })],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::UngrabKeyboard { .. }) => {
                    runtime
                        .input_authority_mut()
                        .ungrab_keyboard(context.namespace, context.client_id);
                    XDispatchResult {
                        response: None,
                        outputs: Vec::new(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::GrabButton {
                    window,
                    event_mask,
                    button,
                    modifiers,
                    owner_events,
                    pointer_mode,
                    keyboard_mode,
                }) => {
                    let outputs = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        runtime
                            .input_authority_mut()
                            .grab_button(
                                context.namespace,
                                crate::XPassiveInputGrab {
                                    owner: context.client_id,
                                    window,
                                    detail: button,
                                    modifiers,
                                    owner_events,
                                    pointer_mode,
                                    keyboard_mode,
                                    event_mask,
                                },
                            )
                            .err()
                            .map(|_| grab_access_error(&context, window))
                            .into_iter()
                            .collect()
                    } else if let Err(error) = runtime.validate_window_access(context.namespace, window) {
                        vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(window.local.raw()).unwrap_or(0)))]
                    } else {
                        runtime
                            .input_authority_mut()
                            .grab_button(
                                context.namespace,
                                crate::XPassiveInputGrab {
                                    owner: context.client_id,
                                    window,
                                    detail: button,
                                    modifiers,
                                    owner_events,
                                    pointer_mode,
                                    keyboard_mode,
                                    event_mask,
                                },
                            )
                            .err()
                            .map(|_| grab_access_error(&context, window))
                            .into_iter()
                            .collect()
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::UngrabButton {
                    window,
                    button,
                    modifiers,
                }) => {
                    runtime.input_authority_mut().ungrab_button(
                        context.namespace,
                        context.client_id,
                        window,
                        button,
                        modifiers,
                    );
                    XDispatchResult {
                        response: None,
                        outputs: Vec::new(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::GrabKey {
                    window,
                    key,
                    modifiers,
                    owner_events,
                    pointer_mode,
                    keyboard_mode,
                }) => {
                    let outputs = match validate_window_or_root_access(runtime, context.namespace, window) {
                        Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(window.local.raw()).unwrap_or(0)))],
                        Ok(()) => runtime
                            .input_authority_mut()
                            .grab_key(
                                context.namespace,
                                crate::XPassiveInputGrab {
                                    owner: context.client_id,
                                    window,
                                    detail: key,
                                    modifiers,
                                    owner_events,
                                    pointer_mode,
                                    keyboard_mode,
                                    event_mask: 3,
                                },
                            )
                            .err()
                            .map(|_| grab_access_error(&context, window))
                            .into_iter()
                            .collect(),
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::UngrabKey {
                    window,
                    key,
                    modifiers,
                }) => {
                    runtime.input_authority_mut().ungrab_key(
                        context.namespace,
                        context.client_id,
                        window,
                        key,
                        modifiers,
                    );
                    XDispatchResult {
                        response: None,
                        outputs: Vec::new(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::AllowEvents { mode, .. }) => {
                    let invalid = runtime
                        .input_authority_mut()
                        .allow_events(context.namespace, context.client_id, mode)
                        .is_err();
                    XDispatchResult {
                        response: None,
                        outputs: invalid
                            .then(|| {
                                XClientOutput::Error(crate::XClientError {
                                    code: XErrorCode::BadValue,
                                    sequence: context.sequence,
                                    resource_id: u32::from(mode),
                                    minor_code: 0,
                                    major_code: context.major_opcode,
                                })
                            })
                            .into_iter()
                            .collect(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::GrabServer) => {
                    let _ = runtime
                        .input_authority_mut()
                        .grab_server(context.namespace, context.client_id);
                    XDispatchResult {
                        response: None,
                        outputs: Vec::new(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::UngrabServer) => {
                    runtime
                        .input_authority_mut()
                        .ungrab_server(context.namespace, context.client_id);
                    XDispatchResult {
                        response: None,
                        outputs: Vec::new(),
                        metadata_candidates: Vec::new(),
                    }
                }
        _ => unreachable!("request family checked before dispatch"),
    })
}
