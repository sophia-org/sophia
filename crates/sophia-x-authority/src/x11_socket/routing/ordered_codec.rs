/// Largest currently encoded XI device record: 80-byte header, one button
/// word, one valuator mask word and two eight-byte scroll valuators. This is
/// codec storage, not permission to admit more output than an owner reserved.
#[cfg(unix)]
const PRIVATE_ORDERED_FRAME_BYTES: usize = 104;

/// One immutable encoded frame. Encoding does not change delivery disposition.
/// The writer must retain its own offset and uncertainty with these bytes.
#[cfg(unix)]
pub(crate) struct PrivateOrderedFrame {
    bytes: [u8; PRIVATE_ORDERED_FRAME_BYTES],
    len: usize,
}

#[cfg(unix)]
impl PrivateOrderedFrame {
    fn zeroed(len: usize) -> Self {
        assert!(len <= PRIVATE_ORDERED_FRAME_BYTES);
        Self {
            bytes: [0; PRIVATE_ORDERED_FRAME_BYTES],
            len,
        }
    }
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
    fn extend_from_slice(&mut self, bytes: &[u8]) {
        let end = self.len + bytes.len();
        self.bytes[self.len..end].copy_from_slice(bytes);
        self.len = end;
    }
}

#[cfg(unix)]
impl AsRef<[u8]> for PrivateOrderedFrame {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

#[cfg(unix)]
fn encode_xi_device_frame(
    byte_order: XByteOrder,
    sequence: u16,
    event_type: u16,
    event: XAuthorityInputEvent,
    event_window: XResourceId,
    child_window: XResourceId,
    event_x: i16,
    event_y: i16,
    flags: u32,
) -> PrivateOrderedFrame {
    let (device, time, detail, root_x, root_y, state) = match event {
        XAuthorityInputEvent::Key(key) => {
            (3, key.time_msec, u32::from(key.keycode), 0, 0, key.state)
        }
        XAuthorityInputEvent::Pointer(pointer) => (
            2,
            pointer.time_msec,
            match pointer.kind {
                XAuthorityPointerEventKind::Button { button, .. } => u32::from(button),
                XAuthorityPointerEventKind::Axis { button, .. } if matches!(event_type, 4 | 5) => {
                    u32::from(button)
                }
                XAuthorityPointerEventKind::Axis { .. } => 0,
                XAuthorityPointerEventKind::Motion => 0,
            },
            pointer.root_x,
            pointer.root_y,
            pointer.state,
        ),
    };
    let mut out = PrivateOrderedFrame::zeroed(80);
    out.bytes[0] = 35;
    out.bytes[1] = crate::X_INPUT_MAJOR_OPCODE;
    write_xi_u16(byte_order, &mut out.bytes[2..4], sequence);
    write_xi_u16(byte_order, &mut out.bytes[8..10], event_type);
    write_xi_u16(byte_order, &mut out.bytes[10..12], device);
    write_xi_u32(byte_order, &mut out.bytes[12..16], time);
    write_xi_u32(byte_order, &mut out.bytes[16..20], detail);
    write_xi_u32(byte_order, &mut out.bytes[20..24], X_SETUP_DEFAULT_ROOT);
    write_xi_u32(
        byte_order,
        &mut out.bytes[24..28],
        u32::try_from(event_window.local.raw()).unwrap_or(0),
    );
    write_xi_u32(
        byte_order,
        &mut out.bytes[28..32],
        u32::try_from(child_window.local.raw()).unwrap_or(0),
    );
    write_xi_u32(
        byte_order,
        &mut out.bytes[32..36],
        (i32::from(root_x) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out.bytes[36..40],
        (i32::from(root_y) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out.bytes[40..44],
        (i32::from(event_x) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out.bytes[44..48],
        (i32::from(event_y) << 16) as u32,
    );
    write_xi_u16(
        byte_order,
        &mut out.bytes[52..54],
        if device == 2 {
            crate::X_INPUT_POINTER_SOURCE_ID
        } else {
            device
        },
    );
    write_xi_u32(byte_order, &mut out.bytes[56..60], flags);
    write_xi_u32(byte_order, &mut out.bytes[72..76], u32::from(state & 0xff));
    let buttons = (1_u8..=5).fold(0_u32, |buttons, button| {
        let core_mask = 1_u16 << (u32::from(button) + 7);
        if state & core_mask != 0 {
            buttons | (1_u32 << button)
        } else {
            buttons
        }
    });
    if buttons != 0 {
        write_xi_u16(byte_order, &mut out.bytes[48..50], 1);
        match byte_order {
            XByteOrder::LittleEndian => out.extend_from_slice(&buttons.to_le_bytes()),
            XByteOrder::BigEndian => out.extend_from_slice(&buttons.to_be_bytes()),
        }
    }
    if let XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
        kind:
            XAuthorityPointerEventKind::Axis {
                horizontal_position_v120,
                vertical_position_v120,
                ..
            },
        ..
    }) = event
        && event_type == 6
        && (horizontal_position_v120.is_some() || vertical_position_v120.is_some())
    {
        write_xi_u16(byte_order, &mut out.bytes[50..52], 1);
        let mut mask = 0u8;
        if horizontal_position_v120.is_some() {
            mask |= 1u8 << u32::from(crate::X_POINTER_HORIZONTAL_SCROLL_VALUATOR);
        }
        if vertical_position_v120.is_some() {
            mask |= 1u8 << u32::from(crate::X_POINTER_VERTICAL_SCROLL_VALUATOR);
        }
        out.extend_from_slice(&[mask, 0, 0, 0]);
        for position in [horizontal_position_v120, vertical_position_v120]
            .into_iter()
            .flatten()
        {
            let fixed = i64::from(position) << 32;
            let integral = ((fixed >> 32) as i32) as u32;
            let fraction = fixed as u32;
            let mut bytes = [0; 8];
            write_xi_u32(byte_order, &mut bytes[..4], integral);
            write_xi_u32(byte_order, &mut bytes[4..], fraction);
            out.extend_from_slice(&bytes);
        }
    }
    let length = u32::try_from((out.len - 32) / 4).unwrap_or(u32::MAX);
    write_xi_u32(byte_order, &mut out.bytes[4..8], length);
    out
}

#[cfg(unix)]
fn encode_xi_crossing_frame(
    byte_order: XByteOrder,
    sequence: u16,
    event_type: u16,
    event: XAuthorityInputEvent,
    event_window: XResourceId,
) -> PrivateOrderedFrame {
    let (device, time, root_x, root_y, event_x, event_y, state) = match event {
        XAuthorityInputEvent::Key(key) => (3, key.time_msec, 0, 0, 0, 0, key.state),
        XAuthorityInputEvent::Pointer(pointer) => (
            2,
            pointer.time_msec,
            pointer.root_x,
            pointer.root_y,
            pointer.event_x,
            pointer.event_y,
            pointer.state,
        ),
    };
    let mut out = PrivateOrderedFrame::zeroed(72);
    out.bytes[0] = 35;
    out.bytes[1] = crate::X_INPUT_MAJOR_OPCODE;
    write_xi_u16(byte_order, &mut out.bytes[2..4], sequence);
    write_xi_u32(byte_order, &mut out.bytes[4..8], 10);
    write_xi_u16(byte_order, &mut out.bytes[8..10], event_type);
    write_xi_u16(byte_order, &mut out.bytes[10..12], device);
    write_xi_u32(byte_order, &mut out.bytes[12..16], time);
    write_xi_u16(
        byte_order,
        &mut out.bytes[16..18],
        if device == 2 {
            crate::X_INPUT_POINTER_SOURCE_ID
        } else {
            device
        },
    );
    out.bytes[18] = 0;
    out.bytes[19] = 3;
    write_xi_u32(byte_order, &mut out.bytes[20..24], X_SETUP_DEFAULT_ROOT);
    write_xi_u32(
        byte_order,
        &mut out.bytes[24..28],
        u32::try_from(event_window.local.raw()).unwrap_or(0),
    );
    write_xi_u32(
        byte_order,
        &mut out.bytes[32..36],
        (i32::from(root_x) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out.bytes[36..40],
        (i32::from(root_y) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out.bytes[40..44],
        (i32::from(event_x) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out.bytes[44..48],
        (i32::from(event_y) << 16) as u32,
    );
    out.bytes[48] = 1;
    out.bytes[49] = 1;
    write_xi_u32(byte_order, &mut out.bytes[64..68], u32::from(state & 0xff));
    out
}
