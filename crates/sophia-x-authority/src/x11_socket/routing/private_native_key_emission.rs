/// Coordinates, target and event forms decided under the exact source guards.
#[derive(Clone, Copy)]
struct KeyPlan {
    target: PrivateResolvedTarget,
    core: bool,
    xkb_details: u16,
}

struct KeyEmission {
    event: XAuthorityKeyEvent,
    plan: KeyPlan,
    root_x: i16,
    root_y: i16,
    before: crate::XkbOrderedState,
    state: crate::XkbOrderedState,
    changed: u16,
}

impl KeyEmission {
    fn new(
        event: XAuthorityKeyEvent,
        plan: KeyPlan,
        position: crate::XPointerObservation,
        before: crate::XkbOrderedState,
        after: crate::XkbOrderedState,
    ) -> Result<Self, PrivateAppliedRefusal> {
        // XI2's group components are bytes; XKB notification base/latched
        // groups are signed shorts. Refuse unrepresentable source state.
        for state in [before, after] {
            let components = state.components();
            for index in [4, 7] {
                u8::try_from(components[index])
                    .map_err(|_| PrivateAppliedRefusal::CoordinateOverflow)?;
            }
            for index in [5, 6] {
                i16::try_from(components[index] as i32)
                    .map_err(|_| PrivateAppliedRefusal::CoordinateOverflow)?;
                if !plan.core {
                    u8::try_from(components[index])
                        .map_err(|_| PrivateAppliedRefusal::CoordinateOverflow)?;
                }
            }
        }
        Ok(Self {
            event,
            plan,
            root_x: position.root_x,
            root_y: position.root_y,
            before,
            state: after,
            changed: after.changed_from(before),
        })
    }

    fn frame_count(&self) -> usize {
        1 + usize::from(self.changed & self.plan.xkb_details != 0)
    }

    fn encode_frame(
        &self,
        index: usize,
        order: XByteOrder,
        sequence: u16,
    ) -> Option<PrivateOrderedFrame> {
        if index >= self.frame_count() {
            return None;
        }
        if index == 1 {
            return Some(self.state_notify(order, sequence));
        }
        let target = self.plan.target;
        let mut frame = if self.plan.core {
            let mut frame = PrivateOrderedFrame::zeroed(32);
            frame.bytes[0] = if self.event.pressed { 2 } else { 3 };
            frame.bytes[1] = self.event.keycode;
            write_xi_u16(order, &mut frame.bytes[2..4], sequence);
            write_xi_u32(order, &mut frame.bytes[4..8], self.event.time_msec);
            write_xi_u32(order, &mut frame.bytes[8..12], X_SETUP_DEFAULT_ROOT);
            write_xi_u32(
                order,
                &mut frame.bytes[12..16],
                target.window.local.raw() as u32,
            );
            write_xi_u32(
                order,
                &mut frame.bytes[16..20],
                target.child.local.raw() as u32,
            );
            for (offset, coordinate) in [
                (20, self.root_x),
                (22, self.root_y),
                (24, target.event_x),
                (26, target.event_y),
            ] {
                write_xi_u16(
                    order,
                    &mut frame.bytes[offset..offset + 2],
                    coordinate as u16,
                );
            }
            write_xi_u16(order, &mut frame.bytes[28..30], self.event.state);
            frame.bytes[30] = 1;
            frame
        } else {
            encode_xi_device_frame(
                order,
                sequence,
                if self.event.pressed { 2 } else { 3 },
                XAuthorityInputEvent::Key(self.event),
                target.window,
                target.child,
                target.event_x,
                target.event_y,
                0,
            )
        };
        if !self.plan.core {
            let before = self.before.components();
            for (offset, component) in [(60, 1), (64, 2), (68, 3), (72, 0)] {
                write_xi_u32(
                    order,
                    &mut frame.bytes[offset..offset + 4],
                    before[component],
                );
            }
            for (offset, component) in [(76, 5), (77, 6), (78, 7), (79, 4)] {
                frame.bytes[offset] = before[component] as u8;
            }
            write_xi_u32(
                order,
                &mut frame.bytes[32..36],
                (i32::from(self.root_x) << 16) as u32,
            );
            write_xi_u32(
                order,
                &mut frame.bytes[36..40],
                (i32::from(self.root_y) << 16) as u32,
            );
        }
        Some(frame)
    }

    fn state_notify(&self, order: XByteOrder, sequence: u16) -> PrivateOrderedFrame {
        // XKBproto.h xkbStateNotify: ptrBtnState at 24, changed at 26,
        // keycode/eventType at 28/29. No allocation or live modifier read.
        let mut frame = PrivateOrderedFrame::zeroed(32);
        let state = self.state.components();
        frame.bytes[0] = crate::X_KEYBOARD_FIRST_EVENT;
        frame.bytes[1] = 2;
        write_xi_u16(order, &mut frame.bytes[2..4], sequence);
        write_xi_u32(order, &mut frame.bytes[4..8], self.event.time_msec);
        frame.bytes[8] = 3;
        for (index, component) in state.iter().take(5).enumerate() {
            frame.bytes[9 + index] = *component as u8;
        }
        write_xi_u16(order, &mut frame.bytes[14..16], state[5] as u16);
        write_xi_u16(order, &mut frame.bytes[16..18], state[6] as u16);
        frame.bytes[18] = state[7] as u8;
        for (index, component) in state.iter().enumerate().skip(8) {
            frame.bytes[11 + index] = *component as u8;
        }
        write_xi_u16(order, &mut frame.bytes[24..26], self.event.state & 0x1f00);
        write_xi_u16(order, &mut frame.bytes[26..28], self.changed);
        frame.bytes[28] = self.event.keycode;
        frame.bytes[29] = if self.event.pressed { 2 } else { 3 };
        frame
    }
}

impl PrivateOrderedEmission {
    fn key(
        hold: &KeyHold,
        delivery: Option<XAuthorityInputDeliveryId>,
        payload: KeyEmission,
    ) -> Self {
        Self {
            origin: hold.origin.clone(),
            incarnation: hold.incarnation.expect("known source key commit"),
            delivery,
            connection: hold.connection(),
            payload: OrderedPayload::Key(payload),
        }
    }
}
