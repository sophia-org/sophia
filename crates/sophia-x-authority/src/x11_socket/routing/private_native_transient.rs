/// A non-ledger effect installed in terminal-owned storage before application.
/// The immutable output belongs to its original request, never a made-up hold.
pub(super) struct Transient {
    emission: Option<PrivateOrderedEmission>,
    committed: bool,
    client: XServerFrontendClientId,
    window: XResourceId,
    event: XAuthorityPointerEvent,
}

impl Transient {
    pub(super) fn take_emission(&mut self) -> Option<PrivateOrderedEmission> {
        if self.committed {
            self.emission.take()
        } else {
            None
        }
    }

    pub(super) fn reached(&self) -> (XServerFrontendClientId, XResourceId, XAuthorityPointerEvent) {
        (self.client, self.window, self.event)
    }
}

impl BaseGuards<'_> {
    /// Freeze motion or the complete axis pair under the same applied/native
    /// guards, then commit mapper/query state once. Queue refusal later cannot
    /// map an axis again or choose another recipient.
    pub(super) fn transient<'connection>(
        &mut self,
        permit: &mut ExecutionPermit<'_>,
        route: &XAuthorityRoutedInput,
        surface: XServerFrontendSurfaceRoute,
        storage: &mut Option<Transient>,
        may_have_applied: &Cell<bool>,
        select_client: impl FnOnce(
            XServerFrontendClientId,
        ) -> Result<
            PrivateAppliedClientRef<'connection>,
            PrivateAppliedRegistryRefusal,
        >,
    ) -> Result<(), Refusal> {
        if permit.identity() != self.origin.identity || surface.namespace != self.origin.namespace {
            return Err(Refusal::ForeignOrigin);
        }
        if storage.is_some() {
            return Err(Refusal::WrongPhase);
        }
        if self.authority.pointer_frozen(self.origin.namespace) {
            return Err(Refusal::PointerFrozen);
        }
        let pointer = self
            .pointers
            .get_mut(&(self.origin.namespace, self.origin.seat))
            .ok_or(Refusal::MissingMapper)?;
        if !self.authority.has_ordered_namespace(self.origin.namespace) {
            return Err(Refusal::MissingQueryScope);
        }
        let grab = self.authority.pointer_grab(self.origin.namespace);
        let recipient = grab.map_or(surface.client, |grab| {
            XServerFrontendClientId::from_raw(grab.owner)
        });
        let client = select_client(recipient).map_err(Refusal::Connection)?;
        if client.client != recipient
            || client.owner.authority != self.origin.identity
            || client.owner.namespace != self.origin.namespace
            || !std::sync::Weak::ptr_eq(
                &client.connection.registry,
                &Arc::downgrade(&self.origin.registry.clients),
            )
        {
            return Err(Refusal::ForeignOrigin);
        }
        let mut selections = client.lock_selections().map_err(Refusal::Connection)?;
        if selections
            .applied_revision
            .and_then(|n| n.checked_add(1))
            .is_none()
        {
            return Err(Refusal::SelectionUnavailable);
        }
        let publication = client.lock_publication().map_err(Refusal::Connection)?;
        let view = publication
            .view(recipient, &selections, &self.authority)
            .map_err(Refusal::Resolution)?;
        let target = grab
            .filter(|grab| !grab.owner_events || recipient != surface.client)
            .map_or(surface.window, |grab| grab.window);
        let modifiers = self
            .authority
            .pointer_query_state(self.origin.namespace)
            .mask
            & 0xff;
        let mut preview = *pointer;
        let mut event = pointer_event(route, 0, false, modifiers | pointer.state());
        let second = match route.request.kind {
            InputEventKind::PointerMotion => {
                event.kind = XAuthorityPointerEventKind::Motion;
                None
            }
            InputEventKind::PointerAxis {
                horizontal_v120,
                vertical_v120,
            } => {
                let axis = preview
                    .map_axis(horizontal_v120, vertical_v120)
                    .ok_or(Refusal::InvalidButton)?;
                event.kind = XAuthorityPointerEventKind::Axis {
                    button: axis.button,
                    pressed: true,
                    horizontal_position_v120: axis.horizontal_position_v120,
                    vertical_position_v120: axis.vertical_position_v120,
                };
                let mut release = event;
                release.kind = XAuthorityPointerEventKind::Axis {
                    button: axis.button,
                    pressed: false,
                    horizontal_position_v120: None,
                    vertical_position_v120: None,
                };
                release.state = modifiers | pointer.axis_release_state(axis.button);
                Some(release)
            }
            _ => return Err(Refusal::WrongPhase),
        };
        let resolve = |mut event: XAuthorityPointerEvent, previous| {
            let plan = match view.pointer(target, event, previous, PrivatePointerSelection::Current)
            {
                Ok(plan) => plan,
                // Smooth motion and either emulated transition have separate
                // selections. An unselected half does not erase the other.
                Err(PrivateAppliedRefusal::NotSelected) => return Ok(None),
                Err(refusal) => return Err(Refusal::Resolution(refusal)),
            };
            let coordinates = selections
                .ordered_coordinates_budget(
                    XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                    plan.surface_window,
                    event.root_x,
                    event.root_y,
                    &PrivateTraversalBudget::new(),
                )
                .map_err(Refusal::Resolution)?;
            event.event_x = coordinates.0;
            event.event_y = coordinates.1;
            Ok::<_, Refusal>(Some((event, plan)))
        };
        let first = resolve(
            event,
            selections.pointer.map(|pointer| pointer.pointer_window),
        )?;
        let second = second
            .map(|event| {
                resolve(
                    event,
                    first
                        .map(|(_, plan)| plan.event_window)
                        .or_else(|| selections.pointer.map(|pointer| pointer.pointer_window)),
                )
            })
            .transpose()?
            .flatten();
        let selected = first
            .or(second)
            .ok_or(Refusal::Resolution(PrivateAppliedRefusal::NotSelected))?;
        let reached = selected
            .1
            .primary_recipient_window()
            .map_err(Refusal::Resolution)?;
        match self
            .origin
            .registry
            .input_recovery
            .bind(route.delivery, client.client)
        {
            Ok(true) => {}
            Ok(false) => return Err(Refusal::DeliveryEnded),
            Err(_) => return Err(Refusal::RecoveryUnavailable),
        }
        let emission = PrivateOrderedEmission {
            origin: self.origin.clone(),
            identity: PrivateEmissionIdentity::Request {
                token: permit.request_token(),
                context: permit.context(),
            },
            delivery: route.delivery,
            connection: RetainedConnection {
                origin: self.origin.clone(),
                selections: client.connection.selections.clone(),
                client: client.client,
                generation: client._admission.generation,
                endpoint: client.endpoint.clone(),
                pointer_tree: None,
            },
            payload: OrderedPayload::PointerPair { first, second },
        };
        *storage = Some(Transient {
            emission: Some(emission),
            committed: false,
            client: client.client,
            window: reached,
            event: selected.0,
        });
        may_have_applied.set(true);
        permit.begin_external_effect().map_err(Refusal::Authority)?;
        *pointer = preview;
        drop(publication);
        // The source applied the whole pair. Publish its final release,
        // retaining the accumulated valuators without leaving a wheel held.
        let mut query = selected.0;
        query.kind = match event.kind {
            XAuthorityPointerEventKind::Axis {
                button,
                horizontal_position_v120,
                vertical_position_v120,
                ..
            } => XAuthorityPointerEventKind::Axis {
                button,
                pressed: false,
                horizontal_position_v120,
                vertical_position_v120,
            },
            other => other,
        };
        query.state = modifiers | pointer.state();
        self.authority.observe_query_input(
            self.origin.namespace,
            surface.window,
            XAuthorityInputEvent::Pointer(query),
        );
        selections.observe_pointer(
            selected.1.surface_window,
            selected.1.event_window,
            query.root_x,
            query.root_y,
            query.event_x,
            query.event_y,
            query.state,
        );
        storage.as_mut().expect("installed before effect").committed = true;
        Ok(())
    }
}
