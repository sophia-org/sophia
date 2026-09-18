#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyReleaseDisposition {
    Unapplied,
    Deliver,
    RecipientTerminationRequired,
}

/// One native keyboard obligation. The executor must install this slot before
/// asking the source to apply, and retain it through terminal handover.
pub(super) struct KeyHold {
    origin: Arc<Origin>,
    selections: Arc<Mutex<XCoreEventSelectionState>>,
    pointer_tree: Option<RetainedPointerTree>,
    client: XServerFrontendClientId,
    generation: u64,
    /// Exactly which endpoint this hold's events are owed to, taken once when
    /// the obligation is installed and never refreshed. Same reason as the
    /// pointer hold: a release answers the endpoint the key went down under.
    endpoint: PrivateEndpointIdentity,
    query_scope: Option<crate::OrderedQueryScopeReceipt>,
    input: Input,
    incarnation: Option<HoldIncarnation>,
    grant: GrantId,
    evdev: u32,
    key: u8,
    plan: KeyPlan,
    /// Nearest registered compositor surface of the selected X window, read
    /// under the caller's surfaces guard and this source's exact tree guard.
    reached_surface: Option<SurfaceId>,
    activation: Option<crate::KeyboardActivation>,
    route_lease: Option<sophia_protocol::ApplicationRouteLeaseIdentity>,
    status: Status,
    proof: Option<Proof>,
    activation_retirement: Option<KeyActivationRetirement>,
    press_emission: Option<PrivateOrderedEmission>,
    release_emission: Option<PrivateOrderedEmission>,
    release_xkb_applied: bool,
    release_disposition: KeyReleaseDisposition,
}

pub(super) struct KeyActivationRetirement {
    origin: Arc<Origin>,
    stamp: crate::KeyboardActivationStamp,
}

impl KeyHold {
    pub(super) fn client(&self) -> XServerFrontendClientId {
        self.client
    }
    pub(super) fn delivered_window(&self) -> XResourceId {
        self.plan.target.window
    }
    pub(super) fn reached_surface(&self) -> Option<SurfaceId> {
        self.reached_surface
    }
    pub(super) fn namespace(&self) -> NamespaceId {
        self.origin.namespace
    }
    /// Positive evidence that this source's release mapping returned. False
    /// does not prove an interrupted source call had no effect.
    pub(super) fn release_xkb_applied(&self) -> bool {
        self.release_xkb_applied
    }
    pub(super) fn release_disposition(&self) -> KeyReleaseDisposition {
        self.release_disposition
    }
    pub(super) fn connection(&self) -> RetainedConnection {
        RetainedConnection {
            origin: self.origin.clone(),
            selections: self.selections.clone(),
            client: self.client,
            generation: self.generation,
            endpoint: self.endpoint.clone(),
            pointer_tree: self.pointer_tree.clone(),
        }
    }
    pub(super) fn input(&self) -> Input {
        self.input
    }
    pub(super) fn incarnation(&self) -> Option<HoldIncarnation> {
        self.incarnation
    }
    pub(super) fn status(&self) -> Status {
        self.status
    }
    pub(super) fn proof(&self) -> Option<&Proof> {
        self.proof.as_ref()
    }
    pub(super) fn take_press_emission(&mut self) -> Option<PrivateOrderedEmission> {
        self.press_emission.take()
    }
    pub(super) fn take_release_emission(&mut self) -> Option<PrivateOrderedEmission> {
        self.release_emission.take()
    }
    pub(super) fn activation_retirement(&self) -> Option<&KeyActivationRetirement> {
        self.activation_retirement.as_ref()
    }

    pub(super) fn complete_shared_activation(
        &mut self,
        receipt: &KeyActivationRetirement,
    ) -> Result<&Proof, Refusal> {
        // A sibling can release before or after its trigger. Absence or a
        // replacement alone proves nothing; the retained source receipt below
        // proves the old activation retired. Other native residuals still
        // cannot be discharged by an activation receipt.
        if !matches!(
            self.status,
            Status::Retained(Residual::KeyboardActivation(
                crate::KeyboardActivationRetirement::StillRequiredByTrigger
                    | crate::KeyboardActivationRetirement::AlreadyAbsent
                    | crate::KeyboardActivationRetirement::Replaced
            ))
        ) {
            return Err(Refusal::WrongPhase);
        }
        if !Arc::ptr_eq(&self.origin, &receipt.origin) {
            return Err(Refusal::ForeignOrigin);
        }
        if self.activation.is_none_or(|activation| {
            activation.trigger().is_none() || activation.stamp() != receipt.stamp
        }) {
            return Err(Refusal::ActivationMismatch);
        }
        self.proof = Some(Proof {
            origin: self.origin.clone(),
            incarnation: self.incarnation.ok_or(Refusal::WrongPhase)?,
            grant: self.grant,
        });
        self.status = Status::NativeReconciled;
        Ok(self.proof.as_ref().expect("source installed key proof"))
    }
}

fn key_state<'a>(
    origin: &Origin,
    keyboards: &'a mut PrivateKeyboards,
) -> Result<&'a mut crate::XkbKeyboardState, Refusal> {
    if !keyboards.answers_for(origin.identity) {
        return Err(Refusal::ForeignOrigin);
    }
    keyboards
        .seats
        .get_mut(&origin.seat)
        .ok_or(Refusal::KeyboardUnavailable)
}

fn validate_key_client(
    origin: &Origin,
    client: &PrivateAppliedClientRef<'_>,
) -> Result<(), Refusal> {
    if client.owner.authority != origin.identity
        || client.owner.namespace != origin.namespace
        || !std::sync::Weak::ptr_eq(
            &client.connection.registry,
            &Arc::downgrade(&origin.registry.clients),
        )
    {
        return Err(Refusal::ForeignOrigin);
    }
    Ok(())
}

impl BaseGuards<'_> {
    /// The first bounded selection is only a lock-acquisition hint. It is
    /// discarded; the complete decision is repeated under both exact selection
    /// guards and the applied publication before binding or applying anything.
    /// At most two selection locks are taken, in client order, then publication.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn press_key<'connection>(
        &mut self,
        permit: &mut ExecutionPermit<'_>,
        capability: DeviceCapability,
        route: &XAuthorityRoutedInput,
        surfaces: &BTreeMap<SurfaceId, XServerFrontendSurfaceRoute>,
        keyboards: &mut PrivateKeyboards,
        storage: &mut Option<KeyHold>,
        may_have_applied: &Cell<bool>,
        select_client: impl Fn(
            XServerFrontendClientId,
        ) -> Result<
            PrivateAppliedClientRef<'connection>,
            PrivateAppliedRegistryRefusal,
        >,
    ) -> Result<(Applied, Option<XAuthorityKeyEvent>), Refusal> {
        if permit.identity() != self.origin.identity
            || capability.source() != permit.source()
            || route.request.seat != self.origin.seat
        {
            return Err(Refusal::ForeignOrigin);
        }
        if storage.is_some() {
            return Err(Refusal::WrongPhase);
        }
        let InputEventKind::Key {
            keycode: evdev,
            pressed: true,
        } = route.request.kind
        else {
            return Err(Refusal::InvalidKey);
        };
        let key = PrivateKeyboards::x_keycode(evdev).ok_or(Refusal::InvalidKey)?;
        let keyboard = key_state(self.origin, keyboards)?;
        if keyboard.physical_key_state(key) != crate::XkbPhysicalKeyState::Released {
            return Err(Refusal::KeyboardUnavailable);
        }
        let before = keyboard
            .ordered_state()
            .ok_or(Refusal::KeyboardUnavailable)?;
        let registry = self
            .origin
            .registry
            .private_applied
            .get()
            .ok_or(Refusal::ForeignOrigin)?;
        let hint = {
            let publication = registry
                .publication
                .lock()
                .map_err(|_| Refusal::Unavailable)?;
            if !publication.published {
                return Err(Refusal::Resolution(PrivateAppliedRefusal::Unpublished));
            }
            if let Some(focus) = publication.focus {
                focus.client
            } else {
                self.authority
                    .keyboard_activation(self.origin.namespace)
                    .map_err(|cause| {
                        Refusal::KeyboardPreparation(match cause {
                            crate::KeyboardActivationRefusal::NamespaceUnprepared => {
                                crate::KeyboardPreparationRefusal::NamespaceUnprepared
                            }
                            crate::KeyboardActivationRefusal::ProvenanceUnavailable => {
                                crate::KeyboardPreparationRefusal::ProvenanceUnavailable
                            }
                        })
                    })?
                    .map(|activation| {
                        XServerFrontendClientId::from_raw(activation.recipient().owner)
                    })
                    .ok_or(Refusal::Resolution(PrivateAppliedRefusal::FocusNotApplied))?
            }
        };
        let focus = select_client(hint).map_err(Refusal::Connection)?;
        validate_key_client(self.origin, &focus)?;
        let recipient = {
            let selections = focus.lock_selections().map_err(Refusal::Connection)?;
            let publication = focus.lock_publication().map_err(Refusal::Connection)?;
            let topology =
                PrivateKeyboardTopology::from_applied(&publication, focus.client, &selections)
                    .map_err(Refusal::KeyboardPreparation)?;
            self.authority
                .prepare_keyboard_press(key, keyboard.modifier_mask(), topology)
                .map_err(Refusal::KeyboardPreparation)?
                .recipient()
                .map_or(focus.client, |grab| {
                    XServerFrontendClientId::from_raw(grab.owner)
                })
        };
        if recipient == focus.client {
            let mut selections = focus.lock_selections().map_err(Refusal::Connection)?;
            let publication = focus.lock_publication().map_err(Refusal::Connection)?;
            self.commit_key(
                permit,
                capability,
                route,
                surfaces,
                keyboard,
                storage,
                may_have_applied,
                &focus,
                &focus,
                None,
                &mut selections,
                &publication,
                before,
                key,
                evdev,
            )
        } else {
            let recipient = select_client(recipient).map_err(Refusal::Connection)?;
            validate_key_client(self.origin, &recipient)?;
            if Arc::ptr_eq(
                &focus.connection.selections,
                &recipient.connection.selections,
            ) {
                return Err(Refusal::ForeignOrigin);
            }
            let (focus_selections, mut selections) = if focus.client.raw() < recipient.client.raw()
            {
                let focus_selections = focus.lock_selections().map_err(Refusal::Connection)?;
                let selections = recipient.lock_selections().map_err(Refusal::Connection)?;
                (focus_selections, selections)
            } else {
                let selections = recipient.lock_selections().map_err(Refusal::Connection)?;
                let focus_selections = focus.lock_selections().map_err(Refusal::Connection)?;
                (focus_selections, selections)
            };
            let publication = focus.lock_publication().map_err(Refusal::Connection)?;
            self.commit_key(
                permit,
                capability,
                route,
                surfaces,
                keyboard,
                storage,
                may_have_applied,
                &focus,
                &recipient,
                Some(&focus_selections),
                &mut selections,
                &publication,
                before,
                key,
                evdev,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_key(
        &mut self,
        permit: &mut ExecutionPermit<'_>,
        capability: DeviceCapability,
        route: &XAuthorityRoutedInput,
        surfaces: &BTreeMap<SurfaceId, XServerFrontendSurfaceRoute>,
        keyboard: &mut crate::XkbKeyboardState,
        storage: &mut Option<KeyHold>,
        may_have_applied: &Cell<bool>,
        focus: &PrivateAppliedClientRef<'_>,
        recipient: &PrivateAppliedClientRef<'_>,
        focus_selections: Option<&XCoreEventSelectionState>,
        selections: &mut XCoreEventSelectionState,
        publication: &PrivateAppliedRoutingState,
        before: crate::XkbOrderedState,
        key: u8,
        evdev: u32,
    ) -> Result<(Applied, Option<XAuthorityKeyEvent>), Refusal> {
        let query_scope = self.authority.ordered_query_scope(self.origin.namespace);
        let topology = PrivateKeyboardTopology::from_applied(
            publication,
            focus.client,
            focus_selections.unwrap_or(selections),
        )
        .map_err(Refusal::KeyboardPreparation)?;
        let position = self
            .authority
            .pointer_query_state(self.origin.namespace)
            .position
            .ok_or(Refusal::MissingQueryScope)?;
        let buttons = self
            .pointers
            .get(&(self.origin.namespace, self.origin.seat))
            .ok_or(Refusal::MissingMapper)?
            .state();
        if selections
            .applied_revision
            .and_then(|n| n.checked_add(1))
            .is_none()
        {
            return Err(Refusal::SelectionUnavailable);
        }
        let prepared = self
            .authority
            .prepare_keyboard_press(key, keyboard.modifier_mask(), topology)
            .map_err(Refusal::KeyboardPreparation)?;
        if prepared
            .recipient()
            .map_or(focus.client.raw(), |grab| grab.owner)
            != recipient.client.raw()
        {
            return Err(Refusal::WrongRecipient);
        }
        let plan = resolve_key_plan(
            publication,
            recipient,
            selections,
            focus_selections.unwrap_or(selections),
            &prepared,
            position,
        )?;
        // Describe the window already selected above. This lookup neither
        // chooses another recipient nor substitutes the pointer's surface.
        let reached_surface = selections
            .ordered_ancestry(plan.target.window)
            .map_err(Refusal::Resolution)?
            .as_slice()
            .iter()
            .find_map(|window| {
                surfaces.iter().find_map(|(surface, route)| {
                    (route.namespace == self.origin.namespace && route.window == *window)
                        .then_some(*surface)
                })
            });
        match self
            .origin
            .registry
            .input_recovery
            .bind(route.delivery, recipient.client)
        {
            Ok(true) => {}
            Ok(false) => return Err(Refusal::DeliveryEnded),
            Err(_) => return Err(Refusal::RecoveryUnavailable),
        }
        let input = Input::key(key).map_err(|_| Refusal::InvalidKey)?;
        *storage = Some(KeyHold {
            origin: self.origin.clone(),
            selections: recipient.connection.selections.clone(),
            pointer_tree: (focus.client != recipient.client).then(|| RetainedPointerTree {
                selections: focus.connection.selections.clone(),
                client: focus.client,
            }),
            client: recipient.client,
            generation: recipient._admission.generation,
            endpoint: recipient.endpoint.clone(),
            query_scope,
            input,
            incarnation: None,
            grant: capability.grant(),
            evdev,
            key,
            plan,
            reached_surface,
            activation: None,
            route_lease: route.route_lease,
            status: Status::PressEntered,
            proof: None,
            activation_retirement: None,
            press_emission: None,
            release_emission: None,
            release_xkb_applied: false,
            release_disposition: KeyReleaseDisposition::Unapplied,
        });
        let prior_application = may_have_applied.replace(true);
        let applied = match permit.press(
            input,
            Recipient {
                recipient: recipient.client.raw(),
                connection_generation: recipient._admission.generation,
            },
        ) {
            Ok(applied) => applied,
            Err(cause) => {
                // A returned common refusal applied no press. Dispose only
                // this unused installed slot; an unwind never reaches here
                // and retains its possibly applied source custody.
                *storage = None;
                may_have_applied.set(prior_application);
                return Err(Refusal::Authority(cause));
            }
        };
        let hold = storage
            .as_mut()
            .expect("source key context installed before effect");
        hold.incarnation = Some(applied.incarnation());
        if !applied.first_press() {
            hold.status = Status::Retained(Residual::IncarnationMismatch);
            return Ok((applied, None));
        }
        hold.activation = prepared.commit();
        let (mapped, modifiers) = keyboard.map_evdev_key(evdev, true).expect("validated key");
        let after = keyboard
            .ordered_state()
            .ok_or(Refusal::KeyboardUnavailable)?;
        let event = XAuthorityKeyEvent {
            keycode: mapped,
            pressed: true,
            state: modifiers | buttons,
            modifiers_after: keyboard.modifier_mask() as u8,
            time_msec: u32::try_from(route.request.time_msec).unwrap_or(u32::MAX),
        };
        self.authority
            .observe_query_modifiers(self.origin.namespace, u16::from(event.modifiers_after));
        observe_key_modifiers(selections, event.modifiers_after);
        hold.status = Status::Held;
        let payload =
            KeyEmission::new(event, plan, position, before, after).map_err(Refusal::Resolution)?;
        hold.press_emission = Some(PrivateOrderedEmission::key(hold, route.delivery, payload));
        Ok((applied, Some(event)))
    }
}

fn observe_key_modifiers(selections: &mut XCoreEventSelectionState, modifiers: u8) {
    let revision = selections.begin_applied_mutation();
    if let Some(pointer) = selections.pointer.as_mut() {
        pointer.mask = (pointer.mask & !0xff) | u16::from(modifiers);
    }
    selections.finish_applied_mutation(revision);
}

include!("private_native_key_routing.rs");
include!("private_native_keyboard_release.rs");
