// The keyboard and pointer maps and controls, the screen saver, the host
// list and the motion history: what a client sets and reads back about
// the server's input. Included by dispatch.rs beside input_discovery.rs;
// one module with it.

fn dispatch_input_control_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
    _properties: &mut XPropertyTable,
) -> XDispatchResult {
    match request {
                XWireRequest::Core(crate::XCoreRequest::GetModifierMapping) => {
                    let (keycodes_per_modifier, keycodes) =
                        runtime.xkb_keymap().core_modifier_mapping();
                    XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::GetModifierMapping {
                        sequence: context.sequence,
                        keycodes_per_modifier,
                        keycodes,
                    })],
                    metadata_candidates: Vec::new(),
                }
                }
                XWireRequest::Core(crate::XCoreRequest::GetPointerMapping) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::GetPointerMapping {
                        sequence: context.sequence,
                        mapping: runtime
                            .input_authority_mut()
                            .pointer_mapping(context.namespace)
                            .as_vec(),
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Core(crate::XCoreRequest::GetKeyboardMapping {
                    first_keycode,
                    count,
                }) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::GetKeyboardMapping {
                        sequence: context.sequence,
                        keysyms_per_keycode: runtime.keyboard_map().keysyms_per_keycode(),
                        keysyms: runtime.keyboard_map().core_mapping(first_keycode, count),
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Core(crate::XCoreRequest::GetKeyboardControl) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::GetKeyboardControl {
                        sequence: context.sequence,
                        keyboard: runtime.controls().keyboard,
                    })],
                    metadata_candidates: Vec::new(),
                },
                // Advisory server controls: what a client sets it reads
                // back, validated as the protocol validates it; nothing
                // here acts on them. The Engine owns pointer acceleration,
                // the session owns key repeat, and no screen is blanked.
                XWireRequest::Core(crate::XCoreRequest::ChangeKeyboardControl(change)) => {
                    let outputs = match runtime.controls_mut().change_keyboard(change) {
                        Ok(()) => Vec::new(),
                        // A led without a mode, or a key without a repeat
                        // mode, is the Match error the protocol names.
                        Err(crate::XKeyboardControlRefusal::Match) => {
                            vec![color_error(context, XErrorCode::BadMatch, 0)]
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::ChangePointerControl {
                    acceleration_numerator,
                    acceleration_denominator,
                    threshold,
                    do_acceleration,
                    do_threshold,
                }) => {
                    runtime.controls_mut().change_pointer(
                        acceleration_numerator,
                        acceleration_denominator,
                        threshold,
                        do_acceleration,
                        do_threshold,
                    );
                    XDispatchResult {
                        response: None,
                        outputs: Vec::new(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::GetPointerControl) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::GetPointerControl {
                        sequence: context.sequence,
                        pointer: runtime.controls().pointer,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Core(crate::XCoreRequest::SetScreenSaver {
                    timeout,
                    interval,
                    prefer_blanking,
                    allow_exposures,
                }) => {
                    runtime.controls_mut().set_screen_saver(
                        timeout,
                        interval,
                        prefer_blanking,
                        allow_exposures,
                    );
                    XDispatchResult {
                        response: None,
                        outputs: Vec::new(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::GetScreenSaver) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::GetScreenSaver {
                        sequence: context.sequence,
                        screen_saver: runtime.controls().screen_saver,
                    })],
                    metadata_candidates: Vec::new(),
                },
                // No motion history is kept, which the protocol allows: a
                // valid window gets an empty reply.
                XWireRequest::Core(crate::XCoreRequest::GetMotionEvents { window, .. }) => {
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
                        vec![XClientOutput::Reply(XClientReply::GetMotionEvents {
                            sequence: context.sequence,
                        })]
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                // Host-based access control is a mechanism this authority
                // does not have: admission is by namespace and peer
                // credentials. The list is empty and enabled, and no client
                // is authorised to change it, which is the protocol's
                // BadAccess.
                XWireRequest::Core(crate::XCoreRequest::ListHosts) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::ListHosts {
                        sequence: context.sequence,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Core(crate::XCoreRequest::ChangeHosts) | XWireRequest::Core(crate::XCoreRequest::SetAccessControl) => XDispatchResult {
                    response: None,
                    outputs: vec![color_error(context, XErrorCode::BadAccess, 0)],
                    metadata_candidates: Vec::new(),
                },
                // The input maps (t166). A pointer mapping is stored and
                // applied to button events; a keyboard mapping rewrites the
                // table clients translate with; a modifier mapping is served
                // only when it is the current one, xkbcommon owning modifier
                // state; QueryKeymap is what the keyboard routing observed.
                XWireRequest::Core(crate::XCoreRequest::SetPointerMapping { ref mapping }) => {
                    let outputs = match crate::XPointerButtonMapping::from_request(mapping) {
                        Err(crate::XPointerMappingRefusal::LengthMismatch(len)) => {
                            vec![color_error(context, XErrorCode::BadValue, u32::from(len))]
                        }
                        Err(crate::XPointerMappingRefusal::DuplicateButton(button)) => {
                            vec![color_error(context, XErrorCode::BadValue, u32::from(button))]
                        }
                        Ok(new_mapping) => {
                            let mut authority = runtime.input_authority_mut();
                            let current = authority.pointer_mapping(context.namespace);
                            let held = authority.held_physical_buttons(context.namespace);
                            if current
                                .changed_buttons(new_mapping)
                                .any(|button| held & (1 << (button - 1)) != 0)
                            {
                                vec![XClientOutput::Reply(XClientReply::MappingStatus {
                                    sequence: context.sequence,
                                    status: 1,
                                })]
                            } else {
                                authority.set_pointer_mapping(context.namespace, new_mapping);
                                // The notice before the reply, as the reference server
                                // orders them: a client reading the reply already holds
                                // the map it names.
                                vec![
                                    XClientOutput::Event(XClientEvent::MappingNotify {
                                        sequence: context.sequence,
                                        request: 2,
                                        first_keycode: 0,
                                        count: 0,
                                    }),
                                    XClientOutput::Reply(XClientReply::MappingStatus {
                                        sequence: context.sequence,
                                        status: 0,
                                    }),
                                ]
                            }
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::ChangeKeyboardMapping {
                    first_keycode,
                    keysyms_per_keycode,
                    ref keysyms,
                }) => {
                    let outputs = match runtime.keyboard_map_mut().change(
                        first_keycode,
                        keysyms_per_keycode,
                        keysyms,
                    ) {
                        Ok(count) => vec![XClientOutput::Event(XClientEvent::MappingNotify {
                            sequence: context.sequence,
                            request: 1,
                            first_keycode,
                            count,
                        })],
                        Err(crate::XKeyboardMapRefusal::KeycodeOutOfRange(keycode)) => {
                            vec![color_error(context, XErrorCode::BadValue, u32::from(keycode))]
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::SetModifierMapping {
                    keycodes_per_modifier,
                    ref keycodes,
                }) => {
                    let width = usize::from(keycodes_per_modifier);
                    let mut requested: [std::collections::BTreeSet<u8>; 8] = Default::default();
                    let mut refused = None;
                    for (modifier, set) in requested.iter_mut().enumerate() {
                        for keycode in keycodes.iter().skip(modifier * width).take(width) {
                            if *keycode == 0 {
                                continue;
                            }
                            if *keycode < runtime.xkb_keymap().min_keycode() {
                                refused = Some(*keycode);
                            }
                            set.insert(*keycode);
                        }
                    }
                    let outputs = if let Some(keycode) = refused {
                        vec![color_error(context, XErrorCode::BadValue, u32::from(keycode))]
                    } else if requested == runtime.xkb_keymap().modifier_sets() {
                        // The notice before the reply, as the reference server
                        // orders them: a client reading the reply already holds
                        // the map it names.
                        vec![
                            XClientOutput::Event(XClientEvent::MappingNotify {
                                sequence: context.sequence,
                                request: 0,
                                first_keycode: 0,
                                count: 0,
                            }),
                            XClientOutput::Reply(XClientReply::MappingStatus {
                                sequence: context.sequence,
                                status: 0,
                            }),
                        ]
                    } else {
                        // Failed: xkbcommon owns the modifier state events
                        // carry, and a map it did not compile cannot be
                        // served honestly. A client reads MappingFailed.
                        vec![XClientOutput::Reply(XClientReply::MappingStatus {
                            sequence: context.sequence,
                            status: 2,
                        })]
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::QueryKeymap) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::QueryKeymap {
                        sequence: context.sequence,
                        keys: runtime.input_authority_mut().pressed_keys(context.namespace),
                    })],
                    metadata_candidates: Vec::new(),
                },
        _ => unreachable!("request family checked before dispatch"),
    }
}
