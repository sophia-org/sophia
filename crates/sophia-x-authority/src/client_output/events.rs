pub fn encode_x_client_event(byte_order: XByteOrder, event: XClientEvent) -> Vec<u8> {
    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
    match event {
        XClientEvent::Key {
            sequence,
            pressed,
            keycode,
            time,
            root,
            event,
            state,
        } => {
            write_event_header(
                byte_order,
                &mut out,
                if pressed { X_KEY_PRESS } else { X_KEY_RELEASE },
                keycode,
                sequence,
            );
            put_u32(byte_order, &mut out[4..8], time);
            put_resource(byte_order, &mut out[8..12], root);
            put_resource(byte_order, &mut out[12..16], event);
            put_resource(byte_order, &mut out[16..20], XResourceId::NONE);
            put_i16(byte_order, &mut out[20..22], 0);
            put_i16(byte_order, &mut out[22..24], 0);
            put_i16(byte_order, &mut out[24..26], 0);
            put_i16(byte_order, &mut out[26..28], 0);
            put_u16(byte_order, &mut out[28..30], state);
            out[30] = 1;
        }
        XClientEvent::Focus {
            sequence,
            focused,
            detail,
            event,
            mode,
        } => {
            write_event_header(
                byte_order,
                &mut out,
                if focused { X_FOCUS_IN } else { X_FOCUS_OUT },
                detail,
                sequence,
            );
            put_resource(byte_order, &mut out[4..8], event);
            out[8] = mode;
        }
        XClientEvent::XkbStateNotify {
            sequence,
            time,
            modifiers,
            changed,
            keycode,
            event_type,
        } => {
            write_event_header(
                byte_order,
                &mut out,
                crate::X_KEYBOARD_FIRST_EVENT,
                2,
                sequence,
            );
            put_u32(byte_order, &mut out[4..8], time);
            out[8] = 3;
            out[9] = modifiers;
            out[10] = modifiers;
            out[16] = modifiers;
            out[18] = modifiers;
            put_u16(byte_order, &mut out[24..26], changed);
            out[26] = keycode;
            out[27] = event_type;
        }
        XClientEvent::PointerMotion {
            sequence,
            time,
            root,
            event,
            root_x,
            root_y,
            event_x,
            event_y,
            state,
        } => write_pointer_event(
            byte_order,
            &mut out,
            X_MOTION_NOTIFY,
            0,
            sequence,
            time,
            root,
            event,
            root_x,
            root_y,
            event_x,
            event_y,
            state,
        ),
        XClientEvent::PointerButton {
            sequence,
            pressed,
            button,
            time,
            root,
            event,
            root_x,
            root_y,
            event_x,
            event_y,
            state,
        } => write_pointer_event(
            byte_order,
            &mut out,
            if pressed {
                X_BUTTON_PRESS
            } else {
                X_BUTTON_RELEASE
            },
            button,
            sequence,
            time,
            root,
            event,
            root_x,
            root_y,
            event_x,
            event_y,
            state,
        ),
        XClientEvent::PointerCrossing {
            sequence,
            entered,
            detail,
            time,
            root,
            event,
            root_x,
            root_y,
            event_x,
            event_y,
            state,
            mode,
            focus,
        } => {
            write_pointer_event(
                byte_order,
                &mut out,
                if entered { 7 } else { 8 },
                detail,
                sequence,
                time,
                root,
                event,
                root_x,
                root_y,
                event_x,
                event_y,
                state,
            );
            out[30] = mode;
            out[31] = 1 | (u8::from(focus) << 1);
        }
        XClientEvent::Expose {
            sequence,
            window,
            x,
            y,
            width,
            height,
            count,
        } => {
            write_event_header(byte_order, &mut out, X_EXPOSE, 0, sequence);
            put_resource(byte_order, &mut out[4..8], window);
            put_u16(byte_order, &mut out[8..10], x);
            put_u16(byte_order, &mut out[10..12], y);
            put_u16(byte_order, &mut out[12..14], width);
            put_u16(byte_order, &mut out[14..16], height);
            put_u16(byte_order, &mut out[16..18], count);
        }
        XClientEvent::GraphicsExpose {
            sequence,
            drawable,
            x,
            y,
            width,
            height,
            minor_opcode,
            count,
            major_opcode,
        } => {
            write_event_header(byte_order, &mut out, X_GRAPHICS_EXPOSE, 0, sequence);
            put_resource(byte_order, &mut out[4..8], drawable);
            put_u16(byte_order, &mut out[8..10], x);
            put_u16(byte_order, &mut out[10..12], y);
            put_u16(byte_order, &mut out[12..14], width);
            put_u16(byte_order, &mut out[14..16], height);
            put_u16(byte_order, &mut out[16..18], minor_opcode);
            put_u16(byte_order, &mut out[18..20], count);
            out[20] = major_opcode;
        }
        XClientEvent::NoExpose {
            sequence,
            drawable,
            minor_opcode,
            major_opcode,
        } => {
            write_event_header(byte_order, &mut out, X_NO_EXPOSE, 0, sequence);
            put_resource(byte_order, &mut out[4..8], drawable);
            put_u16(byte_order, &mut out[8..10], minor_opcode);
            out[10] = major_opcode;
        }
        XClientEvent::VisibilityNotify {
            sequence,
            window,
            state,
        } => {
            write_event_header(byte_order, &mut out, X_VISIBILITY_NOTIFY, 0, sequence);
            put_resource(byte_order, &mut out[4..8], window);
            out[8] = state;
        }
        XClientEvent::ColormapNotify {
            sequence,
            window,
            colormap,
            new,
            state,
        } => {
            write_event_header(byte_order, &mut out, X_COLORMAP_NOTIFY, 0, sequence);
            put_resource(byte_order, &mut out[4..8], window);
            put_u32(byte_order, &mut out[8..12], colormap);
            out[12] = u8::from(new);
            out[13] = state;
        }
        XClientEvent::CreateNotify {
            sequence,
            parent,
            window,
            x,
            y,
            width,
            height,
            border_width,
            override_redirect,
        } => {
            write_event_header(byte_order, &mut out, 16, 0, sequence);
            put_resource(byte_order, &mut out[4..8], parent);
            put_resource(byte_order, &mut out[8..12], window);
            put_i16(byte_order, &mut out[12..14], x);
            put_i16(byte_order, &mut out[14..16], y);
            put_u16(byte_order, &mut out[16..18], width);
            put_u16(byte_order, &mut out[18..20], height);
            put_u16(byte_order, &mut out[20..22], border_width);
            out[22] = u8::from(override_redirect);
        }
        XClientEvent::MapNotify {
            sequence,
            event,
            window,
            override_redirect,
        } => {
            write_event_header(byte_order, &mut out, X_MAP_NOTIFY, 0, sequence);
            put_resource(byte_order, &mut out[4..8], event);
            put_resource(byte_order, &mut out[8..12], window);
            out[12] = u8::from(override_redirect);
        }
        XClientEvent::MapRequest {
            sequence,
            parent,
            window,
        } => {
            write_event_header(byte_order, &mut out, X_MAP_REQUEST, 0, sequence);
            put_resource(byte_order, &mut out[4..8], parent);
            put_resource(byte_order, &mut out[8..12], window);
        }
        XClientEvent::DestroyNotify {
            sequence,
            event,
            window,
        } => {
            write_event_header(byte_order, &mut out, X_DESTROY_NOTIFY, 0, sequence);
            put_resource(byte_order, &mut out[4..8], event);
            put_resource(byte_order, &mut out[8..12], window);
        }
        XClientEvent::UnmapNotify {
            sequence,
            event,
            window,
            from_configure,
        } => {
            write_event_header(byte_order, &mut out, X_UNMAP_NOTIFY, 0, sequence);
            put_resource(byte_order, &mut out[4..8], event);
            put_resource(byte_order, &mut out[8..12], window);
            out[12] = u8::from(from_configure);
        }
        XClientEvent::CirculateNotify {
            sequence,
            event,
            window,
            place,
        } => {
            write_event_header(byte_order, &mut out, X_CIRCULATE_NOTIFY, 0, sequence);
            put_resource(byte_order, &mut out[4..8], event);
            put_resource(byte_order, &mut out[8..12], window);
            out[16] = place;
        }
        XClientEvent::CirculateRequest {
            sequence,
            parent,
            window,
            place,
        } => {
            write_event_header(byte_order, &mut out, X_CIRCULATE_REQUEST, 0, sequence);
            put_resource(byte_order, &mut out[4..8], parent);
            put_resource(byte_order, &mut out[8..12], window);
            out[16] = place;
        }
        XClientEvent::ConfigureRequest {
            sequence,
            stack_mode,
            parent,
            window,
            sibling,
            x,
            y,
            width,
            height,
            border_width,
            value_mask,
        } => {
            write_event_header(byte_order, &mut out, X_CONFIGURE_REQUEST, stack_mode, sequence);
            put_resource(byte_order, &mut out[4..8], parent);
            put_resource(byte_order, &mut out[8..12], window);
            put_resource(byte_order, &mut out[12..16], sibling);
            put_i16(byte_order, &mut out[16..18], x);
            put_i16(byte_order, &mut out[18..20], y);
            put_u16(byte_order, &mut out[20..22], width);
            put_u16(byte_order, &mut out[22..24], height);
            put_u16(byte_order, &mut out[24..26], border_width);
            put_u16(byte_order, &mut out[26..28], value_mask);
        }
        XClientEvent::ResizeRequest {
            sequence,
            window,
            width,
            height,
        } => {
            write_event_header(byte_order, &mut out, X_RESIZE_REQUEST, 0, sequence);
            put_resource(byte_order, &mut out[4..8], window);
            put_u16(byte_order, &mut out[8..10], width);
            put_u16(byte_order, &mut out[10..12], height);
        }
        XClientEvent::ReparentNotify {
            sequence,
            event,
            window,
            parent,
            x,
            y,
            override_redirect,
        } => {
            write_event_header(byte_order, &mut out, X_REPARENT_NOTIFY, 0, sequence);
            put_resource(byte_order, &mut out[4..8], event);
            put_resource(byte_order, &mut out[8..12], window);
            put_resource(byte_order, &mut out[12..16], parent);
            put_i16(byte_order, &mut out[16..18], x);
            put_i16(byte_order, &mut out[18..20], y);
            out[20] = u8::from(override_redirect);
        }
        XClientEvent::MappingNotify {
            sequence,
            request,
            first_keycode,
            count,
        } => {
            write_event_header(byte_order, &mut out, X_MAPPING_NOTIFY, 0, sequence);
            out[4] = request;
            out[5] = first_keycode;
            out[6] = count;
        }
        XClientEvent::ConfigureNotify {
            sequence,
            synthetic,
            event,
            window,
            above_sibling,
            x,
            y,
            width,
            height,
            border_width,
            override_redirect,
        } => {
            write_event_header(
                byte_order,
                &mut out,
                X_CONFIGURE_NOTIFY | if synthetic { 0x80 } else { 0 },
                0,
                sequence,
            );
            put_resource(byte_order, &mut out[4..8], event);
            put_resource(byte_order, &mut out[8..12], window);
            put_u32(
                byte_order,
                &mut out[12..16],
                above_sibling.map(raw_xid).unwrap_or(0),
            );
            put_i16(byte_order, &mut out[16..18], x);
            put_i16(byte_order, &mut out[18..20], y);
            put_u16(byte_order, &mut out[20..22], width);
            put_u16(byte_order, &mut out[22..24], height);
            put_u16(byte_order, &mut out[24..26], border_width);
            out[26] = u8::from(override_redirect);
        }
        XClientEvent::PropertyNotify {
            sequence,
            window,
            atom,
            time,
            new_value,
        } => {
            write_event_header(byte_order, &mut out, X_PROPERTY_NOTIFY, 0, sequence);
            put_resource(byte_order, &mut out[4..8], window);
            put_u32(byte_order, &mut out[8..12], atom);
            put_u32(byte_order, &mut out[12..16], time);
            out[16] = if new_value { PROPERTY_NEW_VALUE } else { 1 };
        }
        XClientEvent::SelectionClear {
            sequence,
            time,
            owner,
            selection,
        } => {
            write_event_header(byte_order, &mut out, 29, 0, sequence);
            put_u32(byte_order, &mut out[4..8], time);
            put_resource(byte_order, &mut out[8..12], owner);
            put_u32(byte_order, &mut out[12..16], selection);
        }
        XClientEvent::SelectionRequest {
            sequence,
            time,
            owner,
            requestor,
            selection,
            target,
            property,
        } => {
            write_event_header(byte_order, &mut out, 30, 0, sequence);
            put_u32(byte_order, &mut out[4..8], time);
            put_resource(byte_order, &mut out[8..12], owner);
            put_resource(byte_order, &mut out[12..16], requestor);
            put_u32(byte_order, &mut out[16..20], selection);
            put_u32(byte_order, &mut out[20..24], target);
            put_u32(byte_order, &mut out[24..28], property);
        }
        XClientEvent::SelectionNotify {
            sequence,
            synthetic,
            time,
            requestor,
            selection,
            target,
            property,
        } => {
            write_event_header(
                byte_order,
                &mut out,
                X_SELECTION_NOTIFY | if synthetic { 0x80 } else { 0 },
                0,
                sequence,
            );
            put_u32(byte_order, &mut out[4..8], time);
            put_resource(byte_order, &mut out[8..12], requestor);
            put_u32(byte_order, &mut out[12..16], selection);
            put_u32(byte_order, &mut out[16..20], target);
            put_u32(byte_order, &mut out[20..24], property);
        }
        XClientEvent::XfixesSelectionNotify {
            sequence,
            subtype,
            window,
            owner,
            selection,
            time,
            selection_time,
        } => {
            // The subtype rides in the detail byte, so one event number covers
            // all three causes and a subscriber reads which happened from
            // byte 1 rather than from a separate event type.
            write_event_header(
                byte_order,
                &mut out,
                crate::X_XFIXES_FIRST_EVENT + crate::X_XFIXES_SELECTION_NOTIFY_SUBEVENT,
                subtype,
                sequence,
            );
            put_resource(byte_order, &mut out[4..8], window);
            put_resource(byte_order, &mut out[8..12], owner);
            put_u32(byte_order, &mut out[12..16], selection);
            put_u32(byte_order, &mut out[16..20], time);
            put_u32(byte_order, &mut out[20..24], selection_time);
        }
        XClientEvent::ClientMessage { sequence, bytes, .. } => {
            out = bytes.to_vec();
            put_u16(byte_order, &mut out[2..4], sequence);
        }
        XClientEvent::ShapeNotify {
            sequence,
            kind,
            window,
            extents,
            shaped,
        } => {
            write_event_header(byte_order, &mut out, crate::X_SHAPE_FIRST_EVENT, kind, sequence);
            put_resource(byte_order, &mut out[4..8], window);
            put_i16(byte_order, &mut out[8..10], extents.x as i16);
            put_i16(byte_order, &mut out[10..12], extents.y as i16);
            put_u16(byte_order, &mut out[12..14], extents.width as u16);
            put_u16(byte_order, &mut out[14..16], extents.height as u16);
            // Time zero, as every other event this server emits: nothing
            // here has a server timestamp to report that a client could
            // meaningfully compare against.
            put_u32(byte_order, &mut out[16..20], 0);
            out[20] = u8::from(shaped);
        }
        XClientEvent::ShmCompletion {
            sequence,
            drawable,
            segment,
            offset,
        } => {
            write_event_header(
                byte_order,
                &mut out,
                crate::X_MIT_SHM_FIRST_EVENT,
                0,
                sequence,
            );
            put_resource(byte_order, &mut out[4..8], drawable);
            put_u16(
                byte_order,
                &mut out[8..10],
                u16::from(crate::X_MIT_SHM_PUT_IMAGE_MINOR_OPCODE),
            );
            out[10] = crate::X_MIT_SHM_MAJOR_OPCODE;
            put_resource(byte_order, &mut out[12..16], segment);
            put_u32(byte_order, &mut out[16..20], offset);
        }
        XClientEvent::PresentConfigureNotify {
            sequence,
            event_id,
            window,
            x,
            y,
            width,
            height,
            pixmap_width,
            pixmap_height,
            pixmap_flags,
        } => {
            out.resize(40, 0);
            out[0] = 35;
            out[1] = crate::X_PRESENT_MAJOR_OPCODE;
            put_u16(byte_order, &mut out[2..4], sequence);
            put_u32(byte_order, &mut out[4..8], 2);
            put_u16(byte_order, &mut out[8..10], 0);
            put_resource(byte_order, &mut out[12..16], event_id);
            put_resource(byte_order, &mut out[16..20], window);
            put_i16(byte_order, &mut out[20..22], x);
            put_i16(byte_order, &mut out[22..24], y);
            put_u16(byte_order, &mut out[24..26], width);
            put_u16(byte_order, &mut out[26..28], height);
            put_i16(byte_order, &mut out[28..30], 0);
            put_i16(byte_order, &mut out[30..32], 0);
            put_u16(byte_order, &mut out[32..34], pixmap_width);
            put_u16(byte_order, &mut out[34..36], pixmap_height);
            put_u32(byte_order, &mut out[36..40], pixmap_flags);
        }
        XClientEvent::PresentCompleteNotify {
            sequence,
            event_id,
            window,
            serial,
            ust,
            msc,
            kind,
            mode,
        } => {
            // XCB appends full_sequence to its in-memory event structure.
            // It is not part of the Present XGE wire record described by
            // present.xml.
            out.resize(40, 0);
            out[0] = 35;
            out[1] = crate::X_PRESENT_MAJOR_OPCODE;
            put_u16(byte_order, &mut out[2..4], sequence);
            put_u32(byte_order, &mut out[4..8], 2);
            put_u16(byte_order, &mut out[8..10], 1);
            out[10] = kind;
            out[11] = mode;
            put_resource(byte_order, &mut out[12..16], event_id);
            put_resource(byte_order, &mut out[16..20], window);
            put_u32(byte_order, &mut out[20..24], serial);
            put_u64(byte_order, &mut out[24..32], ust);
            put_u64(byte_order, &mut out[32..40], msc);
        }
        XClientEvent::PresentIdleNotify {
            sequence,
            event_id,
            window,
            serial,
            pixmap,
            idle_fence,
        } => {
            out[0] = 35;
            out[1] = crate::X_PRESENT_MAJOR_OPCODE;
            put_u16(byte_order, &mut out[2..4], sequence);
            put_u16(byte_order, &mut out[8..10], 2);
            put_resource(byte_order, &mut out[12..16], event_id);
            put_resource(byte_order, &mut out[16..20], window);
            put_u32(byte_order, &mut out[20..24], serial);
            put_resource(byte_order, &mut out[24..28], pixmap);
            put_resource(
                byte_order,
                &mut out[28..32],
                idle_fence.unwrap_or(XResourceId::NONE),
            );
        }
        XClientEvent::RandrScreenChange {
            sequence,
            timestamp,
            config_timestamp,
            root,
            request_window,
            width,
            height,
            mm_width,
            mm_height,
        } => {
            write_event_header(
                byte_order,
                &mut out,
                crate::X_RANDR_FIRST_EVENT,
                1,
                sequence,
            );
            put_u32(byte_order, &mut out[4..8], timestamp);
            put_u32(byte_order, &mut out[8..12], config_timestamp);
            put_resource(byte_order, &mut out[12..16], root);
            put_resource(byte_order, &mut out[16..20], request_window);
            put_u16(byte_order, &mut out[20..22], 0);
            put_u16(byte_order, &mut out[22..24], 0);
            put_u16(byte_order, &mut out[24..26], width);
            put_u16(byte_order, &mut out[26..28], height);
            put_u16(byte_order, &mut out[28..30], mm_width);
            put_u16(byte_order, &mut out[30..32], mm_height);
        }
        XClientEvent::RandrCrtcChange {
            sequence,
            timestamp,
            window,
            crtc,
            mode,
            x,
            y,
            width,
            height,
        } => {
            write_event_header(
                byte_order,
                &mut out,
                crate::X_RANDR_FIRST_EVENT + 1,
                0,
                sequence,
            );
            put_u32(byte_order, &mut out[4..8], timestamp);
            put_resource(byte_order, &mut out[8..12], window);
            put_u32(byte_order, &mut out[12..16], crtc);
            put_u32(byte_order, &mut out[16..20], mode);
            put_u16(byte_order, &mut out[20..22], 1);
            put_i16(byte_order, &mut out[24..26], x);
            put_i16(byte_order, &mut out[26..28], y);
            put_u16(byte_order, &mut out[28..30], width);
            put_u16(byte_order, &mut out[30..32], height);
        }
        XClientEvent::RandrOutputChange {
            sequence,
            timestamp,
            window,
            output,
            crtc,
            mode,
        } => {
            write_event_header(
                byte_order,
                &mut out,
                crate::X_RANDR_FIRST_EVENT + 1,
                1,
                sequence,
            );
            put_u32(byte_order, &mut out[4..8], timestamp);
            put_u32(byte_order, &mut out[8..12], timestamp);
            put_resource(byte_order, &mut out[12..16], window);
            put_u32(byte_order, &mut out[16..20], output);
            put_u32(byte_order, &mut out[20..24], crtc);
            put_u32(byte_order, &mut out[24..28], mode);
            put_u16(byte_order, &mut out[28..30], 1);
            out[30] = 0;
            out[31] = 0;
        }
        XClientEvent::RandrResourceChange {
            sequence,
            timestamp,
            window,
        } => {
            write_event_header(
                byte_order,
                &mut out,
                crate::X_RANDR_FIRST_EVENT + 1,
                5,
                sequence,
            );
            put_u32(byte_order, &mut out[4..8], timestamp);
            put_resource(byte_order, &mut out[8..12], window);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn write_pointer_event(
    byte_order: XByteOrder,
    out: &mut [u8],
    event_type: u8,
    detail: u8,
    sequence: u16,
    time: XTimestamp,
    root: XResourceId,
    event: XResourceId,
    root_x: i16,
    root_y: i16,
    event_x: i16,
    event_y: i16,
    state: u16,
) {
    write_event_header(byte_order, out, event_type, detail, sequence);
    put_u32(byte_order, &mut out[4..8], time);
    put_resource(byte_order, &mut out[8..12], root);
    put_resource(byte_order, &mut out[12..16], event);
    put_resource(byte_order, &mut out[16..20], XResourceId::NONE);
    put_i16(byte_order, &mut out[20..22], root_x);
    put_i16(byte_order, &mut out[22..24], root_y);
    put_i16(byte_order, &mut out[24..26], event_x);
    put_i16(byte_order, &mut out[26..28], event_y);
    put_u16(byte_order, &mut out[28..30], state);
    out[30] = 1;
}
