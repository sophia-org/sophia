/// The source owns the resolved forms and origin; a transport can only encode
/// them. This value is deliberately neither Clone nor Copy. It carries no
/// assertion that any byte was sent or any debt settled.
pub(crate) struct PrivateOrderedEmission {
    origin: Arc<Origin>,
    incarnation: HoldIncarnation,
    delivery: Option<XAuthorityInputDeliveryId>,
    connection: RetainedConnection,
    payload: OrderedPayload,
}

// Both kinds use fixed inline storage. Capacity accounting must charge the
// full enum size; boxing would allocate while the source guards are held.
#[allow(clippy::large_enum_variant)]
enum OrderedPayload {
    Pointer {
        event: XAuthorityPointerEvent,
        plan: PrivateResolvedPointer,
    },
    Key(KeyEmission),
}

impl std::fmt::Debug for PrivateOrderedEmission {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("PrivateOrderedEmission")
            .field("frames", &self.frame_count())
            .finish_non_exhaustive()
    }
}

impl PrivateOrderedEmission {
    fn pointer(
        hold: &Hold,
        delivery: Option<XAuthorityInputDeliveryId>,
        event: XAuthorityPointerEvent,
        plan: PrivateResolvedPointer,
    ) -> Self {
        Self {
            origin: hold.origin.clone(),
            incarnation: hold.incarnation.expect("known source commit"),
            delivery,
            connection: hold.connection(),
            payload: OrderedPayload::Pointer { event, plan },
        }
    }

    pub(crate) fn delivery(&self) -> Option<XAuthorityInputDeliveryId> {
        self.delivery
    }

    pub(crate) fn incarnation(&self) -> HoldIncarnation {
        self.incarnation
    }

    pub(crate) fn connection(&self) -> sophia_input_authority::ConnectionIdentity {
        sophia_input_authority::ConnectionIdentity {
            recipient: self.connection.client.raw(),
            connection_generation: self.connection.generation,
        }
    }

    pub(super) fn answers_for(&self, registry: &XServerFrontendRouteRegistry) -> bool {
        Arc::ptr_eq(&self.origin.registry.clients, &registry.clients)
    }

    pub(crate) fn frame_count(&self) -> usize {
        match &self.payload {
            OrderedPayload::Pointer { plan, .. } => pointer_records(plan).count(),
            OrderedPayload::Key(key) => key.frame_count(),
        }
    }

    /// Only wire byte order and the transport's sequence are supplied here.
    /// The iterator reads fixed, already-decided records and performs no lock,
    /// allocation, selection, modifier update or native/query observation.
    pub(crate) fn encode_frame(
        &self,
        index: usize,
        byte_order: XByteOrder,
        sequence: u16,
    ) -> Option<PrivateOrderedFrame> {
        match &self.payload {
            OrderedPayload::Pointer { event, plan } => pointer_records(plan)
                .nth(index)
                .map(|record| record.encode(*event, byte_order, sequence)),
            OrderedPayload::Key(key) => key.encode_frame(index, byte_order, sequence),
        }
    }
}

fn pointer_records(plan: &PrivateResolvedPointer) -> impl Iterator<Item = EmissionRecord> + '_ {
    // Preserve source-before-master ordering. Crossings precede the
    // corresponding stream's transition; releases have no old crossings.
    plan.crossings[4..]
        .iter()
        .flatten()
        .copied()
        .map(EmissionRecord::Crossing)
        .chain(
            plan.source
                .iter()
                .flatten()
                .copied()
                .map(EmissionRecord::Xi),
        )
        .chain(
            plan.crossings[..4]
                .iter()
                .flatten()
                .copied()
                .map(EmissionRecord::Crossing),
        )
        .chain(plan.core.into_iter().map(EmissionRecord::Core))
        .chain(
            plan.master
                .iter()
                .flatten()
                .copied()
                .map(EmissionRecord::Xi),
        )
}

enum EmissionRecord {
    Core(PrivateResolvedTarget),
    Xi(PrivateResolvedXi),
    Crossing(PrivateResolvedCrossing),
}

impl EmissionRecord {
    fn encode(
        self,
        pointer: XAuthorityPointerEvent,
        byte_order: XByteOrder,
        sequence: u16,
    ) -> PrivateOrderedFrame {
        match self {
            Self::Core(target) => {
                let (kind, detail) = match pointer.kind {
                    XAuthorityPointerEventKind::Motion => (6, 0),
                    XAuthorityPointerEventKind::Button { button, pressed }
                    | XAuthorityPointerEventKind::Axis {
                        button, pressed, ..
                    } => (if pressed { 4 } else { 5 }, button),
                };
                encode_ordered_core_pointer(byte_order, sequence, pointer, target, kind, detail)
            }
            Self::Xi(record) => {
                let mut frame = encode_xi_device_frame(
                    byte_order,
                    sequence,
                    record.event_type,
                    XAuthorityInputEvent::Pointer(pointer),
                    record.target.window,
                    record.target.child,
                    record.target.event_x,
                    record.target.event_y,
                    record.flags,
                );
                write_xi_u16(byte_order, &mut frame.bytes[10..12], record.device);
                frame
            }
            Self::Crossing(record) => {
                let target = record.target;
                if let Some(device) = record.device {
                    let mut event = pointer;
                    event.event_x = target.event_x;
                    event.event_y = target.event_y;
                    let mut frame = encode_xi_crossing_frame(
                        byte_order,
                        sequence,
                        record.event_type,
                        XAuthorityInputEvent::Pointer(event),
                        target.window,
                    );
                    write_xi_u16(byte_order, &mut frame.bytes[10..12], device);
                    write_xi_u32(
                        byte_order,
                        &mut frame.bytes[28..32],
                        target.child.local.raw() as u32,
                    );
                    frame
                } else {
                    let mut frame = encode_ordered_core_pointer(
                        byte_order,
                        sequence,
                        pointer,
                        target,
                        record.event_type as u8,
                        3,
                    );
                    frame.bytes[30] = 0; // NotifyNormal
                    frame.bytes[31] = 3; // same screen, focus
                    frame
                }
            }
        }
    }
}

impl Hold {
    /// Move the actual source result once. A join creates no second emission.
    pub(super) fn take_press_emission(&mut self) -> Option<PrivateOrderedEmission> {
        self.press_emission.take()
    }
    pub(super) fn take_release_emission(&mut self) -> Option<PrivateOrderedEmission> {
        self.release_emission.take()
    }
}

impl Guards<'_> {
    /// Retain the press's forms and resources, but convert this release's root
    /// position independently for every retained event target. No current
    /// selection, focus, grab or hit test chooses any part of this plan.
    fn release_plan(
        &self,
        hold: &Hold,
        event: XAuthorityPointerEvent,
    ) -> Result<PrivateResolvedPointer, PrivateAppliedRefusal> {
        self.validate_hold(hold)
            .map_err(|_| PrivateAppliedRefusal::ForeignOrigin)?;
        let mut plan = hold.plan;
        let budget = PrivateTraversalBudget::new();
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        let convert = |target: &mut PrivateResolvedTarget| -> Result<(), PrivateAppliedRefusal> {
            let (x, y) = self.selections.ordered_coordinates_budget(
                root,
                target.window,
                event.root_x,
                event.root_y,
                &budget,
            )?;
            target.event_x = x;
            target.event_y = y;
            Ok(())
        };
        if let Some(target) = plan.core.as_mut() {
            convert(target)?;
        }
        for record in plan
            .master
            .iter_mut()
            .chain(plan.source.iter_mut())
            .flatten()
        {
            // Holds in this source are buttons. A different stored form is
            // not permission to reinterpret the debt as a release.
            if record.event_type != 4 {
                return Err(PrivateAppliedRefusal::Interrupted);
            }
            record.event_type = 5;
            convert(&mut record.target)?;
        }
        plan.crossings = [None; 6];
        plan.selection_revision = self
            .selections
            .applied_revision
            .ok_or(PrivateAppliedRefusal::Interrupted)?;
        Ok(plan)
    }
}

/// Fixed core record, including the child decided by the source. The ordinary
/// Vec-returning codec remains untouched and is not on this ordered path.
fn encode_ordered_core_pointer(
    order: XByteOrder,
    sequence: u16,
    pointer: XAuthorityPointerEvent,
    target: PrivateResolvedTarget,
    kind: u8,
    detail: u8,
) -> PrivateOrderedFrame {
    let mut frame = PrivateOrderedFrame::zeroed(32);
    frame.bytes[0] = kind;
    frame.bytes[1] = detail;
    write_xi_u16(order, &mut frame.bytes[2..4], sequence);
    write_xi_u32(order, &mut frame.bytes[4..8], pointer.time_msec);
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
        (20, pointer.root_x),
        (22, pointer.root_y),
        (24, target.event_x),
        (26, target.event_y),
    ] {
        write_xi_u16(
            order,
            &mut frame.bytes[offset..offset + 2],
            coordinate as u16,
        );
    }
    write_xi_u16(order, &mut frame.bytes[28..30], pointer.state);
    frame.bytes[30] = 1;
    frame
}
