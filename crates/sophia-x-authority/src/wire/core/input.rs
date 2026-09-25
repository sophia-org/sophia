fn decode_get_input_focus(bytes: &[u8]) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_GET_INPUT_FOCUS, X_GET_INPUT_FOCUS_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::GetInputFocus))
}

fn decode_set_input_focus(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_SET_INPUT_FOCUS, X_SET_INPUT_FOCUS_REQ_LEN, bytes.len())?;
    if bytes[1] > 2 {
        return Err(XWireParseError::InvalidValue(u32::from(bytes[1])));
    }
    Ok(XWireRequest::Core(crate::XCoreRequest::SetInputFocus {
        focus: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        revert_to: bytes[1],
        time: context.byte_order.u32(&bytes[8..12]),
    }))
}

fn decode_get_modifier_mapping(bytes: &[u8]) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_GET_MODIFIER_MAPPING,
        X_GET_MODIFIER_MAPPING_REQ_LEN,
        bytes.len(),
    )?;
    Ok(XWireRequest::Core(crate::XCoreRequest::GetModifierMapping))
}

fn decode_get_pointer_mapping(bytes: &[u8]) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_GET_POINTER_MAPPING,
        X_GET_POINTER_MAPPING_REQ_LEN,
        bytes.len(),
    )?;
    Ok(XWireRequest::Core(crate::XCoreRequest::GetPointerMapping))
}

fn decode_get_keyboard_mapping(bytes: &[u8]) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_GET_KEYBOARD_MAPPING,
        X_GET_KEYBOARD_MAPPING_REQ_LEN,
        bytes.len(),
    )?;
    Ok(XWireRequest::Core(crate::XCoreRequest::GetKeyboardMapping {
        first_keycode: bytes[4],
        count: bytes[5],
    }))
}

/// ChangeKeyboardControl: a mask, then one four-byte value per set bit, in
/// the protocol's order. A set bit outside the eight is a Value error
/// carrying the mask; a value outside its set is a Value error carrying the
/// value. -1 restores a default where the protocol allows it.
fn decode_change_keyboard_control(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(
        X_CHANGE_KEYBOARD_CONTROL,
        X_CHANGE_KEYBOARD_CONTROL_REQ_LEN,
        bytes.len(),
    )?;
    let value_mask = context.byte_order.u32(&bytes[4..8]);
    if value_mask & !X_KEYBOARD_CONTROL_VALUE_MASK != 0 {
        return Err(XWireParseError::InvalidValue(value_mask));
    }
    let expected = X_CHANGE_KEYBOARD_CONTROL_REQ_LEN + 4 * value_mask.count_ones() as usize;
    require_exact_len(X_CHANGE_KEYBOARD_CONTROL, expected, bytes.len())?;
    let mut change = crate::XKeyboardControlChange::default();
    let mut cursor = X_CHANGE_KEYBOARD_CONTROL_REQ_LEN;
    let mut next = || {
        let word = context.byte_order.u32(&bytes[cursor..cursor + 4]);
        cursor += 4;
        word
    };
    let percent = |word: u32| -> Result<i8, XWireParseError> {
        let value = word as i32;
        if !(-1..=100).contains(&value) {
            return Err(XWireParseError::InvalidValue(word));
        }
        Ok(value as i8)
    };
    let at_least_default = |word: u32| -> Result<i16, XWireParseError> {
        let value = word as i32;
        if value < -1 || value > i32::from(i16::MAX) {
            return Err(XWireParseError::InvalidValue(word));
        }
        Ok(value as i16)
    };
    if value_mask & 0x01 != 0 {
        change.key_click_percent = Some(percent(next())?);
    }
    if value_mask & 0x02 != 0 {
        change.bell_percent = Some(percent(next())?);
    }
    if value_mask & 0x04 != 0 {
        change.bell_pitch = Some(at_least_default(next())?);
    }
    if value_mask & 0x08 != 0 {
        change.bell_duration = Some(at_least_default(next())?);
    }
    if value_mask & 0x10 != 0 {
        let led = next();
        if !(1..=32).contains(&led) {
            return Err(XWireParseError::InvalidValue(led));
        }
        change.led = Some(led as u8);
    }
    if value_mask & 0x20 != 0 {
        let mode = next();
        if mode > 1 {
            return Err(XWireParseError::InvalidValue(mode));
        }
        change.led_mode = Some(mode as u8);
    }
    if value_mask & 0x40 != 0 {
        let key = next();
        if !(8..=255).contains(&key) {
            return Err(XWireParseError::InvalidValue(key));
        }
        change.key = Some(key as u8);
    }
    if value_mask & 0x80 != 0 {
        let mode = next();
        if mode > 2 {
            return Err(XWireParseError::InvalidValue(mode));
        }
        change.auto_repeat_mode = Some(mode as u8);
    }
    Ok(XWireRequest::Core(crate::XCoreRequest::ChangeKeyboardControl(change)))
}

/// ChangePointerControl: acceleration as a fraction and a threshold, each
/// applied only when its flag says so. A zero denominator with acceleration
/// asked for, or a value below -1, is the Value error the protocol names.
fn decode_change_pointer_control(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_CHANGE_POINTER_CONTROL,
        X_CHANGE_POINTER_CONTROL_REQ_LEN,
        bytes.len(),
    )?;
    let numerator = context.byte_order.i16(&bytes[4..6]);
    let denominator = context.byte_order.i16(&bytes[6..8]);
    let threshold = context.byte_order.i16(&bytes[8..10]);
    let do_acceleration = bytes[10] != 0;
    let do_threshold = bytes[11] != 0;
    if do_acceleration && (denominator == 0 || numerator < -1 || denominator < -1) {
        return Err(XWireParseError::InvalidValue(if denominator == 0 {
            0
        } else {
            numerator.min(denominator) as u32
        }));
    }
    if do_threshold && threshold < -1 {
        return Err(XWireParseError::InvalidValue(threshold as u32));
    }
    Ok(XWireRequest::Core(crate::XCoreRequest::ChangePointerControl {
        acceleration_numerator: numerator,
        acceleration_denominator: denominator,
        threshold,
        do_acceleration,
        do_threshold,
    }))
}

/// SetScreenSaver: timings at least -1 (the default), and two modes each
/// No, Yes or Default.
fn decode_set_screen_saver(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_SET_SCREEN_SAVER, X_SET_SCREEN_SAVER_REQ_LEN, bytes.len())?;
    let timeout = context.byte_order.i16(&bytes[4..6]);
    let interval = context.byte_order.i16(&bytes[6..8]);
    for value in [timeout, interval] {
        if value < -1 {
            return Err(XWireParseError::InvalidValue(value as u32));
        }
    }
    for mode in [bytes[8], bytes[9]] {
        if mode > 2 {
            return Err(XWireParseError::InvalidValue(u32::from(mode)));
        }
    }
    Ok(XWireRequest::Core(crate::XCoreRequest::SetScreenSaver {
        timeout,
        interval,
        prefer_blanking: bytes[8],
        allow_exposures: bytes[9],
    }))
}

/// ChangeHosts: a mode, a family and an address whose length frames the
/// request exactly. Decoded so the answer is the protocol's BadAccess, not
/// BadRequest; the address itself decides nothing here.
fn decode_change_hosts(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_CHANGE_HOSTS, X_CHANGE_HOSTS_REQ_LEN, bytes.len())?;
    if bytes[1] > 1 {
        return Err(XWireParseError::InvalidValue(u32::from(bytes[1])));
    }
    let address_len = usize::from(context.byte_order.u16(&bytes[6..8]));
    require_exact_len(
        X_CHANGE_HOSTS,
        X_CHANGE_HOSTS_REQ_LEN + ((address_len + 3) & !3),
        bytes.len(),
    )?;
    Ok(XWireRequest::Core(crate::XCoreRequest::ChangeHosts))
}

/// ChangeActivePointerGrab: a cursor, a time and the pointer event mask;
/// a bit outside the pointer events is the Value error the protocol names.
fn decode_change_active_pointer_grab(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_CHANGE_ACTIVE_POINTER_GRAB,
        X_CHANGE_ACTIVE_POINTER_GRAB_REQ_LEN,
        bytes.len(),
    )?;
    let event_mask = context.byte_order.u16(&bytes[12..14]);
    // The pointer event mask: ButtonPress through ButtonMotion, KeymapState
    // is not a pointer event, PointerMotionHint through Button5Motion are.
    const POINTER_EVENT_MASK: u16 = 0x7ffc;
    if event_mask & !POINTER_EVENT_MASK != 0 {
        return Err(XWireParseError::InvalidValue(u32::from(event_mask)));
    }
    Ok(XWireRequest::Core(crate::XCoreRequest::ChangeActivePointerGrab {
        cursor: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        time: context.byte_order.u32(&bytes[8..12]),
        event_mask,
    }))
}

fn decode_grab_button(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_GRAB_BUTTON, X_GRAB_BUTTON_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::GrabButton {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        event_mask: context.byte_order.u16(&bytes[8..10]),
        button: bytes[20],
        modifiers: context.byte_order.u16(&bytes[22..24]),
        owner_events: bytes[1] != 0,
        pointer_mode: bytes[10],
        keyboard_mode: bytes[11],
    }))
}

fn decode_grab_pointer(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_GRAB_POINTER, X_GRAB_POINTER_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::GrabPointer {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        event_mask: context.byte_order.u16(&bytes[8..10]),
        owner_events: bytes[1] != 0,
        pointer_mode: bytes[10],
        keyboard_mode: bytes[11],
        time: context.byte_order.u32(&bytes[20..24]),
    }))
}

fn decode_ungrab_pointer(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_UNGRAB_POINTER, X_UNGRAB_POINTER_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::UngrabPointer {
        time: context.byte_order.u32(&bytes[4..8]),
    }))
}

fn decode_ungrab_button(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_UNGRAB_BUTTON, X_UNGRAB_BUTTON_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::UngrabButton {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        button: bytes[1],
        modifiers: context.byte_order.u16(&bytes[8..10]),
    }))
}

fn decode_grab_keyboard(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_GRAB_KEYBOARD, X_GRAB_KEYBOARD_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::GrabKeyboard {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        owner_events: bytes[1] != 0,
        time: context.byte_order.u32(&bytes[8..12]),
        pointer_mode: bytes[12],
        keyboard_mode: bytes[13],
    }))
}

fn decode_ungrab_keyboard(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_UNGRAB_KEYBOARD, X_UNGRAB_KEYBOARD_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::UngrabKeyboard {
        time: context.byte_order.u32(&bytes[4..8]),
    }))
}

fn decode_grab_key(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_GRAB_KEY, X_GRAB_KEY_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::GrabKey {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        modifiers: context.byte_order.u16(&bytes[8..10]),
        key: bytes[10],
        pointer_mode: bytes[11],
        keyboard_mode: bytes[12],
        owner_events: bytes[1] != 0,
    }))
}

fn decode_ungrab_key(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_UNGRAB_KEY, X_UNGRAB_KEY_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::UngrabKey {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        key: bytes[1],
        modifiers: context.byte_order.u16(&bytes[8..10]),
    }))
}

fn decode_allow_events(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_ALLOW_EVENTS, X_ALLOW_EVENTS_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::AllowEvents {
        mode: bytes[1],
        time: context.byte_order.u32(&bytes[4..8]),
    }))
}

fn decode_grab_server(bytes: &[u8]) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_GRAB_SERVER, X_GRAB_SERVER_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::GrabServer))
}

fn decode_ungrab_server(bytes: &[u8]) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_UNGRAB_SERVER, X_UNGRAB_SERVER_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::UngrabServer))
}
