/// The core keycode range, matching the setup contract in `keyboard.rs`.
const X_VIRTUAL_MASTER_MIN_KEYCODE: u8 = 8;
const X_VIRTUAL_MASTER_MAX_KEYCODE: u8 = u8::MAX;

/// A fixed logical inventory, with no physical device identifiers. The attached
/// source carries pointer classes and is XI2-only; XI1 keeps its core pair.
const X_VIRTUAL_DEVICES: [XVirtualDevice; 3] = [
    XVirtualDevice {
        device_id: crate::X_INPUT_MASTER_POINTER_ID,
        name: "Sophia master pointer",
        kind: XVirtualDeviceKind::Pointer {
            // Derived from the one owner rather than restated, so the master
            // pointer and `GetPointerMapping` cannot disagree about how many
            // buttons exist.
            // Widening a button number; `u16::from` is not const-callable yet.
            button_count: crate::pointer::X_POINTER_BUTTON_COUNT as u16,
        },
    },
    XVirtualDevice {
        device_id: crate::X_INPUT_MASTER_KEYBOARD_ID,
        name: "Sophia master keyboard",
        kind: XVirtualDeviceKind::Keyboard {
            // The keycode range the core setup advertises.
            min_keycode: X_VIRTUAL_MASTER_MIN_KEYCODE,
            max_keycode: X_VIRTUAL_MASTER_MAX_KEYCODE,
        },
    },
    XVirtualDevice {
        device_id: crate::X_INPUT_POINTER_SOURCE_ID,
        name: "Sophia pointer source",
        kind: XVirtualDeviceKind::Pointer {
            button_count: crate::pointer::X_POINTER_BUTTON_COUNT as u16,
        },
    },
];

#[derive(Clone, Copy)]
struct XVirtualDevice {
    device_id: u16,
    name: &'static str,
    kind: XVirtualDeviceKind,
}

#[derive(Clone, Copy)]
enum XVirtualDeviceKind {
    Pointer { button_count: u16 },
    Keyboard { min_keycode: u8, max_keycode: u8 },
}

impl XVirtualDevice {
    /// The XI2 record, carrying the classes only XI2 can express.
    fn xi2_device_info(self, labels: [u32; 4]) -> XXiDeviceInfo {
        let source_id = match self.kind {
            XVirtualDeviceKind::Pointer { .. } => crate::X_INPUT_POINTER_SOURCE_ID,
            XVirtualDeviceKind::Keyboard { .. } => crate::X_INPUT_MASTER_KEYBOARD_ID,
        };
        let (device_type, attachment, classes) = match self.kind {
            XVirtualDeviceKind::Pointer { button_count } => (
                if self.device_id == crate::X_INPUT_POINTER_SOURCE_ID {
                    3
                } else {
                    1
                },
                if self.device_id == crate::X_INPUT_POINTER_SOURCE_ID {
                    crate::X_INPUT_MASTER_POINTER_ID
                } else {
                    crate::X_INPUT_MASTER_KEYBOARD_ID
                },
                vec![
                    XXiDeviceClass::Button {
                        source_id,
                        button_count,
                    },
                    XXiDeviceClass::Valuator {
                        source_id,
                        number: 0,
                        label: labels[0],
                        min: 0,
                        max: i64::from(u16::MAX) << 32,
                        value: 0,
                    },
                    XXiDeviceClass::Valuator {
                        source_id,
                        number: 1,
                        label: labels[1],
                        min: 0,
                        max: i64::from(u16::MAX) << 32,
                        value: 0,
                    },
                    XXiDeviceClass::Valuator {
                        source_id,
                        number: crate::X_POINTER_HORIZONTAL_SCROLL_VALUATOR,
                        label: labels[2],
                        min: 0,
                        max: 0,
                        value: 0,
                    },
                    XXiDeviceClass::Valuator {
                        source_id,
                        number: crate::X_POINTER_VERTICAL_SCROLL_VALUATOR,
                        label: labels[3],
                        min: 0,
                        max: 0,
                        value: 0,
                    },
                    XXiDeviceClass::Scroll {
                        source_id,
                        number: crate::X_POINTER_HORIZONTAL_SCROLL_VALUATOR,
                        scroll_type: 2,
                        flags: 1 << 1,
                        increment: i64::from(120) << 32,
                    },
                    XXiDeviceClass::Scroll {
                        source_id,
                        number: crate::X_POINTER_VERTICAL_SCROLL_VALUATOR,
                        scroll_type: 1,
                        flags: 1 << 1,
                        increment: i64::from(120) << 32,
                    },
                ],
            ),
            XVirtualDeviceKind::Keyboard {
                min_keycode,
                max_keycode,
            } => (
                2,
                crate::X_INPUT_MASTER_POINTER_ID,
                vec![XXiDeviceClass::Key {
                    source_id,
                    keys: (u32::from(min_keycode)..=u32::from(max_keycode)).collect(),
                }],
            ),
        };
        XXiDeviceInfo {
            device_id: self.device_id,
            device_type,
            attachment,
            name: self.name.to_owned(),
            classes,
        }
    }

    /// The XI1 record. A lossy projection by design: XI1 has no scroll class, and a
    /// virtual pointer has no resolution worth stating, so this carries the identity
    /// and the button or key range and nothing invented beyond them. XI2 still
    /// reports the valuators; the asymmetry is deliberate, not an omission.
    ///
    /// `device_use` is load-bearing. `IsXPointer` and `IsXKeyboard` tell an XI1
    /// client these are the core devices and not its to open, which is what keeps it
    /// away from `OpenDevice` and the other XI1 requests Sophia does not implement.
    /// Reporting them as extension devices would answer this request and move the
    /// failure one opcode along.
    fn xi1_device_info(self, device_type: u32) -> XXiLegacyDeviceInfo {
        let (device_use, classes) = match self.kind {
            XVirtualDeviceKind::Pointer { button_count } => (
                crate::X_INPUT_LEGACY_USE_POINTER,
                vec![XXiLegacyDeviceClass::Button { button_count }],
            ),
            XVirtualDeviceKind::Keyboard {
                min_keycode,
                max_keycode,
            } => (
                crate::X_INPUT_LEGACY_USE_KEYBOARD,
                vec![XXiLegacyDeviceClass::Key {
                    min_keycode,
                    max_keycode,
                }],
            ),
        };
        XXiLegacyDeviceInfo {
            device_id: u8::try_from(self.device_id).unwrap_or(0),
            device_type,
            device_use,
            name: self.name.to_owned(),
            classes,
        }
    }

    /// The atom name XI1 reports as this device's type.
    const fn legacy_type_name(self) -> &'static str {
        match self.kind {
            XVirtualDeviceKind::Pointer { .. } => "MOUSE",
            XVirtualDeviceKind::Keyboard { .. } => "KEYBOARD",
        }
    }
}

fn dispatch_x_input_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::Xi(crate::XInputRequest::XiGetExtensionVersion)
            | XWireRequest::Xi(crate::XInputRequest::XiListInputDevices)
            | XWireRequest::Xi(crate::XInputRequest::XiQueryPointer { .. })
            | XWireRequest::Xi(crate::XInputRequest::XiGrabDevice { .. })
            | XWireRequest::Xi(crate::XInputRequest::XiUngrabDevice { .. })
            | XWireRequest::Xi(crate::XInputRequest::XiGetClientPointer)
            | XWireRequest::Xi(crate::XInputRequest::XiDeviceBell)
            | XWireRequest::Xi(crate::XInputRequest::XiChangeCursor { .. })
            | XWireRequest::Extension(crate::XExtensionRequest::GeQueryVersion { .. })
            | XWireRequest::Xi(crate::XInputRequest::XiQueryVersion { .. })
            | XWireRequest::Xi(crate::XInputRequest::XiQueryDevice { .. })
            | XWireRequest::Xi(crate::XInputRequest::XiSelectEvents { .. })
            | XWireRequest::Xi(crate::XInputRequest::XiGetFocus { .. })
            | XWireRequest::Xi(crate::XInputRequest::XiGetProperty)
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::Xi(crate::XInputRequest::XiGetExtensionVersion) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::XiGetExtensionVersion {
                        sequence: context.sequence,
                        server_major: 2,
                        server_minor: 0,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Xi(crate::XInputRequest::XiQueryPointer { device_id: crate::X_INPUT_POINTER_SOURCE_ID, .. }) =>
                    xi_device_error(context, crate::X_INPUT_POINTER_SOURCE_ID, XErrorCode::XiBadDevice,
                        crate::X_INPUT_QUERY_POINTER_MINOR_OPCODE),
                // Independent source grabs would detach a logical device from the
                // Engine-owned route. This frontend explicitly denies that policy.
                XWireRequest::Xi(crate::XInputRequest::XiGrabDevice { device_id: crate::X_INPUT_POINTER_SOURCE_ID, .. }) =>
                    xi_device_error(context, crate::X_INPUT_POINTER_SOURCE_ID, XErrorCode::BadAccess,
                        crate::X_INPUT_GRAB_DEVICE_MINOR_OPCODE),
                XWireRequest::Xi(crate::XInputRequest::XiQueryPointer { window, .. }) => {
                    let output = match runtime.query_pointer(context.namespace, window) {
                        Ok(pointer) => XClientOutput::Reply(XClientReply::XiQueryPointer {
                            sequence: context.sequence,
                            root: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                            child: pointer.child,
                            root_x: pointer.root_x,
                            root_y: pointer.root_y,
                            win_x: pointer.win_x,
                            win_y: pointer.win_y,
                            buttons: u32::from((pointer.mask >> 8) & 0x1f) << 1,
                            modifiers: pointer.mask & 0xff,
                        }),
                        Err(error) => XClientOutput::Error(x_error_from_runtime(error,
                            context.sequence, context.major_opcode,
                            crate::X_INPUT_QUERY_POINTER_MINOR_OPCODE.into(),
                            u32::try_from(window.local.raw()).unwrap_or(0))),
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Xi(crate::XInputRequest::XiUngrabDevice { device_id, .. }) => {
                    match device_id {
                        2 => runtime
                            .input_authority_mut()
                            .ungrab_pointer(context.namespace, context.client_id),
                        3 => runtime
                            .input_authority_mut()
                            .ungrab_keyboard(context.namespace, context.client_id),
                        _ => {}
                    }
                    XDispatchResult {
                        response: None,
                        outputs: Vec::new(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Xi(crate::XInputRequest::XiGrabDevice {
                    window,
                    cursor,
                    device_id,
                    pointer_mode,
                    keyboard_mode,
                    owner_events,
                    event_mask,
                    ..
                }) => {
                    let mut xi_event_mask = [0; 8];
                    for (target, source) in xi_event_mask.iter_mut().zip(&event_mask) {
                        *target = *source;
                    }
                    let status = if device_id != 2
                        || validate_window_or_root_access(runtime, context.namespace, window).is_err()
                        || cursor.is_some_and(|cursor| {
                            runtime
                                .validate_cursor_access(context.namespace, cursor)
                                .is_err()
                        })
                    {
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
                                    event_mask: 0,
                                    xi_event_mask,
                                    xi_event_mask_words: event_mask.len() as u8,
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
                XWireRequest::Xi(crate::XInputRequest::XiGetClientPointer) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::XiGetClientPointer {
                        sequence: context.sequence,
                        device_id: 2,
                    })],
                    metadata_candidates: Vec::new(),
                },
                // DeviceBell has no server-side state in Sophia. Accepting the bounded
                // legacy XInput request matches an X server with its bell disabled.
                XWireRequest::Xi(crate::XInputRequest::XiDeviceBell) => XDispatchResult {
                    response: None,
                    outputs: Vec::new(),
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Xi(crate::XInputRequest::XiChangeCursor { window, cursor }) => {
                    let result = validate_window_or_root_access(
                        runtime,
                        context.namespace,
                        window,
                    )
                        .and_then(|()| {
                            cursor.map_or(Ok(()), |cursor| {
                                runtime.validate_cursor_access(context.namespace, cursor)
                            })
                        });
                    let resource_id = cursor.map_or_else(
                        || u32::try_from(window.local.raw()).unwrap_or(0),
                        |cursor| u32::try_from(cursor.local.raw()).unwrap_or(0),
                    );
                    let outputs = result
                        .err()
                        .map(|error| {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_INPUT_CHANGE_CURSOR_MINOR_OPCODE),
                                resource_id))
                        })
                        .into_iter()
                        .collect();
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Extension(crate::XExtensionRequest::GeQueryVersion { .. }) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::GeQueryVersion {
                        sequence: context.sequence,
                        major_version: 1,
                        minor_version: 0,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Xi(crate::XInputRequest::XiQueryVersion { .. }) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::XiQueryVersion {
                        sequence: context.sequence,
                        major_version: 2,
                        minor_version: 1,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Xi(crate::XInputRequest::XiQueryDevice { device_id }) => {
                    let mut labels = [X_ATOM_NONE; 4];
                    for (label, name) in labels.iter_mut().zip([
                        "Rel X", "Rel Y", "Rel Horiz Scroll", "Rel Vert Scroll",
                    ]) {
                        let Ok(Some(atom)) = atoms.intern(name, false) else {
                            return Handled(xi_device_error(context, device_id,
                                XErrorCode::BadAlloc, crate::X_INPUT_QUERY_DEVICE_MINOR_OPCODE));
                        };
                        *label = atom;
                    }
                    let pointer = runtime.input_authority_mut().pointer_query_state(context.namespace);
                    let devices = X_VIRTUAL_DEVICES
                        .iter()
                        .filter(|device| {
                            device_id == 0
                                || (device_id == 1 && device.device_id != crate::X_INPUT_POINTER_SOURCE_ID)
                                || device_id == device.device_id
                        })
                        .map(|device| {
                            let mut info = device.xi2_device_info(labels);
                            for class in &mut info.classes {
                                if let XXiDeviceClass::Valuator { number, value, .. } = class {
                                    *value = match *number {
                                        0 => i64::from(pointer.position.map_or(0, |p| p.root_x)) << 32,
                                        1 => i64::from(pointer.position.map_or(0, |p| p.root_y)) << 32,
                                        crate::X_POINTER_HORIZONTAL_SCROLL_VALUATOR => i64::from(pointer.horizontal_scroll_v120) << 32,
                                        crate::X_POINTER_VERTICAL_SCROLL_VALUATOR => i64::from(pointer.vertical_scroll_v120) << 32,
                                        _ => *value,
                                    };
                                }
                            }
                            info
                        })
                        .collect();
                    XDispatchResult {
                        response: None,
                        outputs: vec![XClientOutput::Reply(XClientReply::XiQueryDevice {
                            sequence: context.sequence,
                            devices,
                        })],
                        metadata_candidates: Vec::new(),
                    }
                }
                // XI1's device enumeration. Sophia advertises XInputExtension and
                // answers the XI1 version handshake, so refusing the enumeration that
                // follows it left an advertised extension half-implemented; a real
                // session failed on the resulting BadRequest storm.
                XWireRequest::Xi(crate::XInputRequest::XiListInputDevices) => {
                    let devices = X_VIRTUAL_DEVICES
                        .iter()
                        .filter(|device| device.device_id < 128)
                        .map(|device| {
                            let device_type = atoms
                                .intern(device.legacy_type_name(), false)
                                .ok()
                                .flatten()
                                .unwrap_or(X_ATOM_NONE);
                            device.xi1_device_info(device_type)
                        })
                        .collect();
                    XDispatchResult {
                        response: None,
                        outputs: vec![XClientOutput::Reply(XClientReply::XiListInputDevices {
                            sequence: context.sequence,
                            devices,
                        })],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Xi(crate::XInputRequest::XiSelectEvents { window, masks }) => {
                    let outputs = (window.local.raw() != u64::from(X_SETUP_DEFAULT_ROOT))
                        .then(|| {
                            runtime
                                .validate_window_access(context.namespace, window)
                                .err()
                        })
                        .flatten()
                        .map(|error| {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_INPUT_SELECT_EVENTS_MINOR_OPCODE),
                                u32::try_from(window.local.raw()).unwrap_or(0)))
                        })
                        .into_iter()
                        .collect::<Vec<_>>();
                    if outputs.is_empty() {
                        runtime.input_authority_mut().select_xi_events(
                            context.namespace,
                            context.client_id,
                            window,
                            &masks,
                        );
                    }
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Xi(crate::XInputRequest::XiGetFocus { .. }) => {
                    let (focus, _) = runtime.input_focus(context.namespace);
                    XDispatchResult {
                        response: None,
                        outputs: vec![XClientOutput::Reply(XClientReply::XiGetFocus {
                            sequence: context.sequence,
                            focus,
                        })],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Xi(crate::XInputRequest::XiGetProperty) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::XiGetProperty {
                        sequence: context.sequence,
                    })],
                    metadata_candidates: Vec::new(),
                },
        _ => unreachable!("request family checked before dispatch"),
    })
}

fn xi_device_error(
    context: XDispatchContext,
    device: u16,
    code: XErrorCode,
    minor: u8,
) -> XDispatchResult {
    XDispatchResult {
        response: None,
        outputs: vec![XClientOutput::Error(crate::XClientError {
            code,
            sequence: context.sequence,
            resource_id: u32::from(device),
            minor_code: u16::from(minor),
            major_code: context.major_opcode,
        })],
        metadata_candidates: Vec::new(),
    }
}
