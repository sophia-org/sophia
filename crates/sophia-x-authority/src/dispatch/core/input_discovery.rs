fn dispatch_core_input_discovery_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
    _properties: &mut XPropertyTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::GetInputFocus
            | XWireRequest::SetInputFocus { .. }
            | XWireRequest::GetModifierMapping
            | XWireRequest::GetPointerMapping
            | XWireRequest::GetKeyboardMapping { .. }
            | XWireRequest::GetKeyboardControl
            | XWireRequest::Bell
            | XWireRequest::ForceScreenSaver { .. }
            | XWireRequest::WarpPointer { .. }
            | XWireRequest::TranslateCoordinates { .. }
            | XWireRequest::QueryPointer { .. }
            | XWireRequest::QueryExtension { .. }
            | XWireRequest::ListExtensions
            | XWireRequest::NoOperation
            | XWireRequest::QueryBestSize { .. }
            | XWireRequest::QueryColors { .. }
            | XWireRequest::CreateColormap { .. }
            | XWireRequest::FreeColormap { .. }
            | XWireRequest::ColormapRequest { .. }
            | XWireRequest::CopyColormapAndFree { .. }
            | XWireRequest::ListInstalledColormaps { .. }
            | XWireRequest::AllocNamedColor { .. }
            | XWireRequest::LookupColor { .. }
            | XWireRequest::AllocColor { .. }
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::GetInputFocus => {
                    let (focus, revert_to) = runtime.input_focus(context.namespace);
                    XDispatchResult {
                        response: None,
                        outputs: vec![XClientOutput::Reply(XClientReply::GetInputFocus {
                            sequence: context.sequence,
                            focus,
                            revert_to,
                        })],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::SetInputFocus {
                    focus,
                    revert_to,
                    time,
                } => {
                    let (previous, _) = runtime.input_focus(context.namespace);
                    let applied =
                        x11_apply_focus_request(runtime, context, focus, revert_to, time);
                    let events = if matches!(applied, XFocusRequestOutcome::Applied) {
                        core_focus_transition_events(runtime, context.namespace, previous, focus)
                    } else {
                        Vec::new()
                    };
                    input_focus_dispatch_result(context, focus, applied, events)
                }
                XWireRequest::GetModifierMapping => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::GetModifierMapping {
                        sequence: context.sequence,
                        keycodes_per_modifier: 2,
                        keycodes: vec![50, 62, 66, 0, 37, 105, 64, 108, 77, 0, 0, 0, 133, 134, 0, 0],
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::GetPointerMapping => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::GetPointerMapping {
                        sequence: context.sequence,
                        mapping: crate::pointer::x_pointer_button_mapping(),
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::GetKeyboardMapping {
                    first_keycode,
                    count,
                } => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::GetKeyboardMapping {
                        sequence: context.sequence,
                        keysyms_per_keycode: 2,
                        keysyms: runtime.xkb_keymap().core_mapping(first_keycode, count),
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::GetKeyboardControl => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::GetKeyboardControl {
                        sequence: context.sequence,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Bell => XDispatchResult {
                    response: None,
                    outputs: Vec::new(),
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::ForceScreenSaver { mode } => {
                    // Reset is 0 and Activate is 1. This host blanks nothing
                    // and has no idle timer, so both are accepted and move
                    // no state; the suite's per-test reset needs exactly
                    // that. A mode outside the pair is the Value error the
                    // protocol names, reporting the mode as its value.
                    const X_SCREEN_SAVER_ACTIVATE: u8 = 1;
                    XDispatchResult {
                        response: None,
                        outputs: (mode > X_SCREEN_SAVER_ACTIVATE)
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
                XWireRequest::TranslateCoordinates {
                    source,
                    destination,
                    src_x,
                    src_y,
                } => {
                    let output =
                        if let Err(error) = runtime.validate_drawable_access(context.namespace, source) {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(source.local.raw()).unwrap_or(0)))
                        } else if let Err(error) =
                            runtime.validate_drawable_access(context.namespace, destination)
                        {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(destination.local.raw()).unwrap_or(0)))
                        } else {
                            // The point moves between two windows' coordinate
                            // spaces, which is only the identity when both
                            // sit at the same place. Echoing the input back
                            // told every client its window was at the screen
                            // origin, and a toolkit that positions a menu
                            // from its parent's screen position put the menu
                            // wherever the window was not.
                            let translated = runtime
                                .window_root_position(source)
                                .zip(runtime.window_root_position(destination))
                                .map(|(from, to)| {
                                    // Widened for the arithmetic and clamped
                                    // back: the reply's fields are sixteen
                                    // bits, and a window far off a large
                                    // desktop can put the sum outside them.
                                    let translate = |value: i16, from: i32, to: i32| {
                                        i32::from(value)
                                            .saturating_add(from)
                                            .saturating_sub(to)
                                            .clamp(i32::from(i16::MIN), i32::from(i16::MAX))
                                            as i16
                                    };
                                    (
                                        translate(src_x, from.0, to.0),
                                        translate(src_y, from.1, to.1),
                                    )
                                });
                            match translated {
                                Some((dst_x, dst_y)) => XClientOutput::Reply(
                                    XClientReply::TranslateCoordinates {
                                        sequence: context.sequence,
                                        same_screen: true,
                                        // Which child of the destination holds
                                        // the point is not reported. A client
                                        // that needs it asks the pointer
                                        // instead, and answering with a guess
                                        // would be worse than answering none.
                                        child: None,
                                        dst_x,
                                        dst_y,
                                    },
                                ),
                                None => XClientOutput::Error(crate::XClientError {
                                    code: XErrorCode::BadWindow,
                                    sequence: context.sequence,
                                    resource_id: u32::try_from(source.local.raw())
                                        .unwrap_or(0),
                                    minor_code: 0,
                                    major_code: context.major_opcode,
                                }),
                            }
                        };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::WarpPointer {
                    source,
                    destination,
                    src_x,
                    src_y,
                    src_width,
                    src_height,
                    dst_x,
                    dst_y,
                } => {
                    // Beside QueryPointer, which reads the position this
                    // writes. A warp that names a window that does not exist
                    // is a Window error; one whose source rectangle does not
                    // hold the pointer is a silent no-op, which the protocol
                    // asks for and is not a refusal.
                    let outputs = match runtime.warp_pointer(
                        context.namespace,
                        source,
                        destination,
                        src_x,
                        src_y,
                        src_width,
                        src_height,
                        dst_x,
                        dst_y,
                    ) {
                        Ok(()) => Vec::new(),
                        Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(source.local.raw().max(destination.local.raw()))
                                .unwrap_or(0),
                        ))],
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::QueryPointer { window } => {
                    let output = match runtime.query_pointer(context.namespace, window) {
                        Ok(pointer) => XClientOutput::Reply(XClientReply::QueryPointer {
                            sequence: context.sequence,
                            root: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                            child: pointer.child,
                            root_x: pointer.root_x,
                            root_y: pointer.root_y,
                            win_x: pointer.win_x,
                            win_y: pointer.win_y,
                            mask: pointer.mask,
                        }),
                        Err(error) => XClientOutput::Error(x_error_from_runtime(error,
                            context.sequence, context.major_opcode, 0,
                            u32::try_from(window.local.raw()).unwrap_or(0))),
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::QueryExtension { name } => {
                    let extension = extension_query_result(&name, context.injection);
                    if !extension.present {
                        // The only record of what this server was asked for and
                        // could not provide. A client asks once per extension
                        // per connection and then quietly does without, so
                        // without this line a missing extension is invisible
                        // until someone notices the consequence -- which is how
                        // XF86VidMode went unnoticed until a browser logged a
                        // failure once per frame.
                        //
                        // The name is the client's own bytes, so it is bounded
                        // and stripped to printable ASCII before it reaches a
                        // log a person will read.
                        tracing::info!(
                            "sophia_x11_authority_extension schema=1 status=absent client={} name={:?}",
                            context.client_id,
                            loggable_extension_name(&name),
                        );
                    }
                    XDispatchResult {
                        response: None,
                        outputs: vec![XClientOutput::Reply(XClientReply::QueryExtension {
                            sequence: context.sequence,
                            present: extension.present,
                            major_opcode: extension.major_opcode,
                            first_event: extension.first_event,
                            first_error: extension.first_error,
                        })],
                        metadata_candidates: Vec::new(),
                    }
                }
                // Does nothing, and says nothing: no reply, no error. It still
                // consumes a sequence number, which the connection assigns
                // before dispatch, so the request after it completes against
                // the sequence the client expects.
                XWireRequest::NoOperation => XDispatchResult {
                    response: None,
                    outputs: Vec::new(),
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::ListExtensions => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::ListExtensions {
                        sequence: context.sequence,
                        names: crate::dispatch::advertised_extension_names(context.injection),
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::QueryBestSize {
                    class,
                    drawable,
                    width,
                    height,
                } => {
                    // The drawable must exist, and a tile or stipple size is
                    // only meaningful for one with pixels: an InputOnly
                    // window is a Match error for those two classes.
                    if let Err(error) = runtime.validate_drawable_access(context.namespace, drawable)
                    {
                        core_resource_validation_error(
                            context,
                            error,
                            XErrorCode::BadDrawable,
                            drawable,
                        )
                    } else if class != 0 && runtime.window_is_input_only(drawable) {
                        core_resource_validation_error(
                            context,
                            XAuthorityRuntimeError::InvalidSurface,
                            XErrorCode::BadMatch,
                            drawable,
                        )
                    } else {
                        XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Reply(XClientReply::QueryBestSize {
                                sequence: context.sequence,
                                width,
                                height,
                            })],
                            metadata_candidates: Vec::new(),
                        }
                    }
                }
                XWireRequest::QueryColors { colormap, pixels } => {
                    let output = match runtime.colormap_visual(context.namespace, colormap) {
                        Err(_) => color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        ),
                        Ok(visual_id) => {
                            let visual = x_true_color_visual(visual_id)
                                .expect("registered colormaps must name advertised visuals");
                            if let Some(invalid) = pixels
                                .iter()
                                .copied()
                                .find(|pixel| visual.query(*pixel).is_none())
                            {
                                color_error(context, XErrorCode::BadValue, invalid)
                            } else {
                                XClientOutput::Reply(XClientReply::QueryColors {
                                    sequence: context.sequence,
                                    colors: pixels
                                        .into_iter()
                                        .map(|pixel| {
                                            visual
                                                .query(pixel)
                                                .expect("pixels were validated before encoding")
                                        })
                                        .collect(),
                                })
                            }
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::CreateColormap {
                    alloc,
                    colormap,
                    window,
                    visual,
                } => {
                    let output = if alloc > 1 {
                        Some(color_error(
                            context,
                            XErrorCode::BadValue,
                            u32::from(alloc),
                        ))
                    } else if runtime.resource_id_in_use(colormap) {
                        Some(color_error(
                            context,
                            XErrorCode::BadIdChoice,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        ))
                    } else if window.local.raw() != u64::from(X_SETUP_DEFAULT_ROOT)
                        && runtime
                            .validate_window_access(context.namespace, window)
                            .is_err()
                    {
                        Some(color_error(
                            context,
                            XErrorCode::BadWindow,
                            u32::try_from(window.local.raw()).unwrap_or(0),
                        ))
                    } else if x_true_color_visual(visual).is_none() || alloc != 0 {
                        Some(color_error(context, XErrorCode::BadMatch, visual))
                    } else {
                        match runtime.create_colormap(
                            context.namespace,
                            colormap,
                            visual,
                            1,
                        ) {
                            Ok(()) => None,
                            Err(XColormapError::DuplicateId) => Some(color_error(
                                context,
                                XErrorCode::BadIdChoice,
                                u32::try_from(colormap.local.raw()).unwrap_or(0),
                            )),
                            Err(XColormapError::UnknownVisual) => {
                                Some(color_error(context, XErrorCode::BadMatch, visual))
                            }
                            Err(XColormapError::Access(_)) => Some(color_error(
                                context,
                                XErrorCode::BadAccess,
                                u32::try_from(colormap.local.raw()).unwrap_or(0),
                            )),
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs: output.into_iter().collect(),
                        metadata_candidates: Vec::new(),
                    }
                }
                // A TrueColor visual has no allocable cells and a read-only
                // colormap, so these requests are answered rather than served.
                // They are decoded at all so the answer is the protocol's own
                // error: a client meeting BadRequest may treat it as fatal,
                // and these arrive on ordinary teardown paths.
                // A static visual has no allocations to move: the copy is a
                // new colormap on the source's visual, and the source keeps
                // what it never had. The id is the client's to choose once.
                XWireRequest::CopyColormapAndFree { colormap, source } => {
                    let output = match runtime.colormap_visual(context.namespace, source) {
                        Err(_) => Some(color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(source.local.raw()).unwrap_or(0),
                        )),
                        Ok(visual) => match runtime.create_colormap(
                            context.namespace,
                            colormap,
                            visual,
                            1,
                        ) {
                            Ok(()) => None,
                            Err(XColormapError::DuplicateId) => Some(color_error(
                                context,
                                XErrorCode::BadIdChoice,
                                u32::try_from(colormap.local.raw()).unwrap_or(0),
                            )),
                            Err(XColormapError::UnknownVisual) => {
                                Some(color_error(context, XErrorCode::BadMatch, visual))
                            }
                            Err(XColormapError::Access(_)) => Some(color_error(
                                context,
                                XErrorCode::BadAccess,
                                u32::try_from(colormap.local.raw()).unwrap_or(0),
                            )),
                        },
                    };
                    XDispatchResult {
                        response: None,
                        outputs: output.into_iter().collect(),
                        metadata_candidates: Vec::new(),
                    }
                }
                // The one installed colormap is the default, always: the
                // setup advertises one installed map at most and at least,
                // and GetWindowAttributes reports every window's installed.
                XWireRequest::ListInstalledColormaps { window } => {
                    let outputs = if window.local.raw() != u64::from(X_SETUP_DEFAULT_ROOT)
                        && runtime
                            .validate_window_access(context.namespace, window)
                            .is_err()
                    {
                        vec![color_error(
                            context,
                            XErrorCode::BadWindow,
                            u32::try_from(window.local.raw()).unwrap_or(0),
                        )]
                    } else {
                        vec![XClientOutput::Reply(XClientReply::ListInstalledColormaps {
                            sequence: context.sequence,
                            colormaps: vec![crate::X_SETUP_DEFAULT_COLORMAP],
                        })]
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::ColormapRequest { kind, colormap } => {
                    let known = runtime.colormap_visual(context.namespace, colormap).is_ok();
                    let outputs = if !known {
                        vec![color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        )]
                    } else {
                        match kind {
                            crate::XColormapRequestKind::AllocCells
                            | crate::XColormapRequestKind::AllocPlanes => {
                                vec![color_error(context, XErrorCode::BadAlloc, 0)]
                            }
                            crate::XColormapRequestKind::StoreColors
                            | crate::XColormapRequestKind::StoreNamedColor => {
                                vec![color_error(context, XErrorCode::BadAccess, 0)]
                            }
                            // Installing is a no-op on a visual whose colormap
                            // is always installed, and freeing colours that
                            // were never allocated is not an error.
                            crate::XColormapRequestKind::Install
                            | crate::XColormapRequestKind::Uninstall
                            | crate::XColormapRequestKind::FreeColors => Vec::new(),
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::FreeColormap { colormap } => {
                    let outputs = match runtime.free_colormap(context.namespace, colormap) {
                        Ok(()) => Vec::new(),
                        Err(_) => vec![color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        )],
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::AllocNamedColor { colormap, ref name }
                | XWireRequest::LookupColor { colormap, ref name } => {
                    let lookup = matches!(request, XWireRequest::LookupColor { .. });
                    let output = match runtime.colormap_visual(context.namespace, colormap) {
                        Err(_) => color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        ),
                        Ok(visual_id) => match x_lookup_color_name(name) {
                            None => color_error(context, XErrorCode::BadName, 0),
                            Some(exact) => {
                                let visual = x_true_color_visual(visual_id)
                                    .expect("registered colormaps must name advertised visuals");
                                let screen = visual.screen_color(exact);
                                XClientOutput::Reply(if lookup {
                                    XClientReply::LookupColor {
                                        sequence: context.sequence, exact, screen,
                                    }
                                } else { XClientReply::AllocNamedColor {
                                    sequence: context.sequence,
                                    pixel: visual.pixel(screen),
                                    exact,
                                    screen,
                                } })
                            }
                        },
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::AllocColor {
                    colormap,
                    red,
                    green,
                    blue,
                } => {
                    let output = match runtime.colormap_visual(context.namespace, colormap) {
                        Err(_) => color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        ),
                        Ok(visual_id) => {
                            let visual = x_true_color_visual(visual_id)
                                .expect("registered colormaps must name advertised visuals");
                            let screen = visual.screen_color(XColorRgb16 { red, green, blue });
                            XClientOutput::Reply(XClientReply::AllocColor {
                                sequence: context.sequence,
                                pixel: visual.pixel(screen),
                                red: screen.red,
                                green: screen.green,
                                blue: screen.blue,
                            })
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
        _ => unreachable!("request family checked before dispatch"),
    })
}

fn color_error(context: XDispatchContext, code: XErrorCode, resource_id: u32) -> XClientOutput {
    XClientOutput::Error(crate::XClientError {
        code,
        sequence: context.sequence,
        resource_id,
        minor_code: 0,
        major_code: context.major_opcode,
    })
}

/// What a focus request did, which X11 makes three-valued rather than two.
///
/// A request whose timestamp falls outside the window is neither applied nor
/// an error. The protocol says it has no effect, so the client is owed
/// nothing at all: no reply, no error, and no focus events. Without a third
/// value that case is indistinguishable from success and the client is told
/// the focus moved when it did not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XFocusRequestOutcome {
    Applied,
    Ignored,
    Refused(XAuthorityRuntimeError),
}

/// The focus events a reversion made during this request owes the client.
///
/// Selection is not consulted here: this layer decides the requesting
/// client's own copy, and the routing filter drops what the window did not
/// ask for, exactly as it does for the events a focus request generates.
fn focus_reversion_outputs(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
) -> Vec<XClientOutput> {
    runtime
        .take_focus_reversions()
        .into_iter()
        .filter(|(namespace, _)| *namespace == context.namespace)
        .flat_map(|(_, events)| events)
        .map(|event| {
            XClientOutput::Event(XClientEvent::Focus {
                sequence: context.sequence,
                focused: event.focused,
                detail: event.detail,
                event: event.window,
                mode: crate::X_FOCUS_MODE_NORMAL,
            })
        })
        .collect()
}

/// The whole of what SetInputFocus decides, in the order X11 states it.
///
/// Errors come first and are reported whatever the timestamp says, so a
/// client that names an impossible revert_to or a window nobody created
/// learns so even when its clock is also wrong. Only a request that would
/// otherwise have succeeded is then measured against the server time, and
/// only one that survives both moves the focus.
pub(crate) fn x11_apply_focus_request(
    runtime: &mut XAuthorityRuntime,
    context: XDispatchContext,
    focus: XResourceId,
    revert_to: u8,
    time: crate::XTimestamp,
) -> XFocusRequestOutcome {
    if let Err(error) = runtime.validate_input_focus(context.namespace, focus, revert_to) {
        return XFocusRequestOutcome::Refused(error);
    }
    let Some(effective) = runtime.focus_time_admits(context.namespace, time, context.server_time)
    else {
        return XFocusRequestOutcome::Ignored;
    };
    match runtime.set_input_focus(context.namespace, focus, revert_to) {
        Ok(()) => {
            runtime.note_focus_change(context.namespace, effective);
            XFocusRequestOutcome::Applied
        }
        Err(error) => XFocusRequestOutcome::Refused(error),
    }
}

/// Shared exact core reply/event construction. The caller supplies the result
/// of the actual effect producer; this routine changes no focus state.
/// The focus events a transition owes, over the whole window chain.
///
/// The private path resolves this in the record producer, which can see each
/// window's event selection. This path cannot, so it emits the whole chain
/// and lets the routing filter drop what the client did not select, which is
/// the same arrangement every other lifecycle event here uses.
pub(crate) fn core_focus_transition_events(
    runtime: &XAuthorityRuntime,
    namespace: NamespaceId,
    previous: XResourceId,
    focus: XResourceId,
) -> Vec<crate::XFocusTransitionEvent> {
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let ancestry = |window: XResourceId| runtime.window_ancestry_chain(window);
    let chains = crate::XFocusChains {
        root,
        // The pointer's window when one has been observed. Without an
        // observation the pointer is on the root, which is where it is when
        // it is in no other window.
        pointer: runtime.pointer_window(namespace).unwrap_or(root),
        ancestry: &ancestry,
    };
    crate::x_focus_transition_events(
        crate::XFocusTarget::from_resource(previous),
        crate::XFocusTarget::from_resource(focus),
        &chains,
    )
}

pub(crate) fn input_focus_dispatch_result(
    context: XDispatchContext,
    focus: XResourceId,
    applied: XFocusRequestOutcome,
    events: Vec<crate::XFocusTransitionEvent>,
) -> XDispatchResult {
                    let outputs = match applied {
                        XFocusRequestOutcome::Refused(error) => vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(focus.local.raw()).unwrap_or(0)))],
                        XFocusRequestOutcome::Ignored => Vec::new(),
                        XFocusRequestOutcome::Applied => events
                            .into_iter()
                            .map(|event| {
                                XClientOutput::Event(XClientEvent::Focus {
                                    sequence: context.sequence,
                                    focused: event.focused,
                                    detail: event.detail,
                                    event: event.window,
                                    mode: crate::X_FOCUS_MODE_NORMAL,
                                })
                            })
                            .collect(),
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
}
