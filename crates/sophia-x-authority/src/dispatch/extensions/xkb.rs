fn dispatch_xkb_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::XkbUseExtension { .. }
            | XWireRequest::XkbGetMap { .. }
            | XWireRequest::XkbGetCompatMap { .. }
            | XWireRequest::XkbGetIndicatorMap { .. }
            | XWireRequest::XkbGetState
            | XWireRequest::XkbLatchLockState { .. }
            | XWireRequest::XkbGetControls
            | XWireRequest::XkbGetNames { .. }
            | XWireRequest::XkbGetDeviceInfo { .. }
            | XWireRequest::XkbSelectEvents { .. }
            | XWireRequest::XkbPerClientFlags { .. }
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::XkbUseExtension { .. } => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::XkbUseExtension {
                        sequence: context.sequence,
                        supported: true,
                        server_major: 1,
                        server_minor: 0,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::XkbGetMap { full, partial } => {
                    // Preserve every component requested by the client. Components
                    // outside Sophia's reduced types/symbols/modifier map are valid
                    // empty sections, represented by their zero counts in the reply.
                    let present = full | partial;
                    let keysyms = runtime.keyboard_map().xkb_keysyms();
                    let modifier_map = runtime.xkb_keymap().modifier_map();
                    XDispatchResult {
                        response: None,
                        outputs: vec![XClientOutput::Reply(XClientReply::XkbGetMap {
                            sequence: context.sequence,
                            present,
                            keysyms,
                            modifier_map,
                        })],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::XkbGetCompatMap { device_spec } => xkb_empty_device_reply(
                    context,
                    device_spec,
                    crate::X_KEYBOARD_GET_COMPAT_MAP_MINOR_OPCODE,
                    |sequence, device_id| XClientReply::XkbGetCompatMap {
                        sequence,
                        device_id,
                    },
                ),
                XWireRequest::XkbGetIndicatorMap { device_spec } => xkb_empty_device_reply(
                    context,
                    device_spec,
                    crate::X_KEYBOARD_GET_INDICATOR_MAP_MINOR_OPCODE,
                    |sequence, device_id| XClientReply::XkbGetIndicatorMap {
                        sequence,
                        device_id,
                    },
                ),
                XWireRequest::XkbGetState => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::XkbGetState {
                        sequence: context.sequence,
                        modifiers: 0,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::XkbGetControls => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::XkbGetControls {
                        sequence: context.sequence,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::XkbGetNames { which } => {
                    let config = runtime.xkb_keymap().config();
                    let layout = if config.variant.is_empty() {
                        config.layout.clone()
                    } else {
                        format!("{}({})", config.layout, config.variant)
                    };
                    let components = [
                        (1, config.rules.clone()),
                        (2, config.model.clone()),
                        (4, layout.clone()),
                        (8, layout),
                        (16, "complete".to_owned()),
                        (32, "complete".to_owned()),
                    ];
                    let present = which & 0x3fff;
                    let component_atoms = components
                        .iter()
                        .filter(|(mask, _)| present & mask != 0)
                        .filter_map(|(_, name)| atoms.intern(name, false).ok().flatten())
                        .collect();
                    let type_atoms: Vec<u32> = ["ONE_LEVEL", "TWO_LEVEL", "ALPHABETIC", "KEYPAD"]
                        .iter()
                        .filter_map(|name| atoms.intern(name, false).ok().flatten())
                        .collect();
                    // NAMED, NOT None. Two levels per type, matching the
                    // numLevels XkbGetMap advertises. The spec permits atom
                    // None for an unnamed level and we used to send it, which
                    // is why a client that walks these names and asks the
                    // server for each one -- xdotool does, through libxdo --
                    // handed us None, got BadAtom, and was killed by libX11
                    // for asking. A real server names them, so the case never
                    // arose there. Falling back to the type's own atom keeps
                    // one name per level even if interning is exhausted, so
                    // the count can never disagree with the type count.
                    const LEVEL_NAMES: [[&str; 2]; 4] = [
                        ["Any", "Any"],
                        ["Base", "Shift"],
                        ["Base", "Caps"],
                        ["Base", "Number"],
                    ];
                    let level_atoms: Vec<u32> = LEVEL_NAMES
                        .iter()
                        .zip(&type_atoms)
                        .flat_map(|(names, type_atom)| {
                            names.map(|name| {
                                atoms
                                    .intern(name, false)
                                    .ok()
                                    .flatten()
                                    .unwrap_or(*type_atom)
                            })
                        })
                        .collect();
                    let key_names = (runtime.xkb_keymap().min_keycode()
                        ..=runtime.xkb_keymap().max_keycode())
                        .map(|keycode| {
                            let name = format!("I{keycode:03}");
                            let mut bytes = [0; 4];
                            bytes.copy_from_slice(name.as_bytes());
                            bytes
                        })
                        .collect();
                    XDispatchResult {
                        response: None,
                        outputs: vec![XClientOutput::Reply(XClientReply::XkbGetNames {
                            sequence: context.sequence,
                            which: present,
                            min_keycode: runtime.xkb_keymap().min_keycode(),
                            max_keycode: runtime.xkb_keymap().max_keycode(),
                            component_atoms,
                            type_atoms,
                            level_atoms,
                            key_names,
                        })],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::XkbGetDeviceInfo {
                    device_spec,
                    wanted,
                } => {
                    const XKB_USE_CORE_KBD: u16 = 0x0100;
                    let outputs = if matches!(device_spec, XKB_USE_CORE_KBD | 3) {
                        vec![XClientOutput::Reply(XClientReply::XkbGetDeviceInfo {
                            sequence: context.sequence,
                            device_id: 3,
                            supported: 0,
                            unsupported: wanted,
                        })]
                    } else {
                        vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: u32::from(device_spec),
                            minor_code: crate::X_KEYBOARD_GET_DEVICE_INFO_MINOR_OPCODE.into(),
                            major_code: context.major_opcode,
                        })]
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::XkbLatchLockState {
                    affect_mod_locks,
                    mod_locks,
                    lock_group,
                    group_lock,
                    affect_mod_latches,
                    mod_latches,
                    latch_group,
                    group_latch,
                } => {
                    // SATISFIED WHEN THE STATE ASKED FOR ALREADY HOLDS, and
                    // refused when it does not, rather than accepted and
                    // dropped. This keymap has one group and XkbGetState
                    // reports no modifier latched or locked, so a request to
                    // be on group zero with nothing latched or locked -- what
                    // a client sends to put the keyboard in a known state
                    // before synthesising keys, which is how xdotool begins
                    // every type -- is already true and is answered by doing
                    // nothing. Naming another group is out of range for a
                    // one-group keymap. Asking to hold a modifier down is a
                    // legal request this instance has no state to honour, and
                    // BadImplementation says that rather than pretending.
                    let names_absent_group =
                        (lock_group && group_lock != 0) || (latch_group && group_latch != 0);
                    let sets_modifier = affect_mod_locks & mod_locks != 0
                        || affect_mod_latches & mod_latches != 0;
                    let refusal = if names_absent_group {
                        Some((XErrorCode::BadValue, u32::from(group_lock.max(
                            u8::try_from(group_latch).unwrap_or(u8::MAX),
                        ))))
                    } else if sets_modifier {
                        Some((XErrorCode::BadImplementation, 0))
                    } else {
                        None
                    };
                    XDispatchResult {
                        response: None,
                        outputs: refusal.map_or_else(Vec::new, |(code, resource_id)| {
                            vec![XClientOutput::Error(crate::XClientError {
                                code,
                                sequence: context.sequence,
                                resource_id,
                                minor_code: crate::X_KEYBOARD_LATCH_LOCK_STATE_MINOR_OPCODE.into(),
                                major_code: context.major_opcode,
                            })]
                        }),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::XkbSelectEvents { .. } => XDispatchResult {
                    response: None,
                    outputs: Vec::new(),
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::XkbPerClientFlags { change, value } => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::XkbPerClientFlags {
                        sequence: context.sequence,
                        supported: change,
                        value: value & change,
                    })],
                    metadata_candidates: Vec::new(),
                },
        _ => unreachable!("request family checked before dispatch"),
    })
}
