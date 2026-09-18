/// Native pointer/key effects and their source-produced cleanup evidence. The
/// nested module seals phases and proof fields from the executor: callers may
/// retain a context or ask its status, but cannot mark a projection repaired.
#[cfg(unix)]
#[allow(dead_code)] // The guarded consumer integration owns the first production callers.
mod private_native {
    use super::*;
    use sophia_input_authority::{
        Applied, AuthorityIdentity, DeviceCapability, ExecutionPermit, GrantId, HoldIncarnation,
        Input, Recipient, RegistrationError, ReleaseOutcome, SettlementBit,
    };
    use std::cell::Cell;
    use std::sync::MutexGuard;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Refusal {
        ForeignOrigin,
        MissingMapper,
        MissingQueryScope,
        Unavailable,
        InvalidButton,
        InvalidKey,
        KeyboardUnavailable,
        PointerFrozen,
        WrongPhase,
        WrongRecipient,
        ActivationMismatch,
        SelectionUnavailable,
        DeliveryEnded,
        RecoveryUnavailable,
        Preparation(crate::PointerPreparationRefusal),
        KeyboardPreparation(crate::KeyboardPreparationRefusal),
        Resolution(PrivateAppliedRefusal),
        Connection(PrivateAppliedRegistryRefusal),
        Authority(RegistrationError),
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(super) enum Residual {
        Interrupted,
        MissingMapper,
        MissingQueryScope,
        SelectionUnavailable,
        ExternalLease,
        Synchronous,
        Activation(crate::PointerActivationRetirement),
        IncarnationMismatch,
        KeyboardUnavailable,
        KeyboardActivation(crate::KeyboardActivationRetirement),
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(super) enum Status {
        PressEntered,
        Held,
        ReleaseEntered,
        Retained(Residual),
        NativeReconciled,
    }

    struct Origin {
        controller: PrivateAuthorityController,
        registry: XServerFrontendRouteRegistry,
        identity: AuthorityIdentity,
        namespace: NamespaceId,
        seat: SeatId,
    }

    /// Constructed before exposing producers. Holds clone this fixed owner;
    /// no per-effect allocation or replacement-origin argument is needed.
    #[derive(Clone)]
    pub(super) struct Owner(Arc<Origin>);

    impl Owner {
        pub(super) fn prepare(
            controller: &PrivateAuthorityController,
            registry: &XServerFrontendRouteRegistry,
            namespace: NamespaceId,
            seat: SeatId,
        ) -> Result<Self, Refusal> {
            let (identity, bound_seat) = controller
                .under_common_as_origin(|authority, issuer| {
                    authority
                        .authority_identity(issuer)
                        .map(|identity| (identity, issuer.binding().seat()))
                })
                .map_err(|_| Refusal::Unavailable)?
                .map_err(|_| Refusal::ForeignOrigin)?;
            if seat != bound_seat {
                return Err(Refusal::ForeignOrigin);
            }
            let applied = registry
                .private_applied
                .get()
                .ok_or(Refusal::ForeignOrigin)?;
            if applied.authority != identity || applied.namespace != namespace {
                return Err(Refusal::ForeignOrigin);
            }
            if !applied.ready.load(Ordering::Acquire) {
                return Err(Refusal::Unavailable);
            }
            Ok(Self(Arc::new(Origin {
                controller: controller.clone(),
                registry: registry.clone(),
                identity,
                namespace,
                seat,
            })))
        }

        /// Take the ranked native guards before choosing a recipient. The
        /// callback in BaseGuards::press selects its exact connection witness
        /// while the same pointer/grab guards remain held.
        pub(super) fn lock_base(&self) -> Result<BaseGuards<'_>, Refusal> {
            Ok(BaseGuards {
                origin: &self.0,
                pointers: self
                    .0
                    .registry
                    .pointer_state
                    .lock()
                    .map_err(|_| Refusal::Unavailable)?,
                authority: self
                    .0
                    .registry
                    .input_authority
                    .lock()
                    .map_err(|_| Refusal::Unavailable)?,
            })
        }

        /// Common, admission, clients and surfaces (if needed) precede this
        /// acquisition. The exact witness remains borrowed through the whole
        /// press. No copied selections or caller-provided mapper are accepted.
        pub(super) fn lock_for_connection<'a>(
            &'a self,
            client: &'a PrivateAppliedClientRef<'_>,
        ) -> Result<Guards<'a>, Refusal> {
            if client.owner.authority != self.0.identity
                || client.owner.namespace != self.0.namespace
                || !std::sync::Weak::ptr_eq(
                    &client.connection.registry,
                    &Arc::downgrade(&self.0.registry.clients),
                )
            {
                return Err(Refusal::ForeignOrigin);
            }
            self.lock(
                &client.connection.selections,
                client.client,
                client._admission.generation,
                None,
            )
        }

        /// Cleanup retains the real connection projection even after its
        /// registration disappears. It does not perform a fresh route lookup.
        pub(super) fn lock_for_release<'a>(
            &'a self,
            connection: &'a RetainedConnection,
        ) -> Result<Guards<'a>, Refusal> {
            if !Arc::ptr_eq(&self.0, &connection.origin) {
                return Err(Refusal::ForeignOrigin);
            }
            self.lock(
                &connection.selections,
                connection.client,
                connection.generation,
                connection.pointer_tree.as_ref(),
            )
        }

        fn lock<'a>(
            &'a self,
            selection_owner: &'a Arc<Mutex<XCoreEventSelectionState>>,
            client: XServerFrontendClientId,
            generation: u64,
            pointer_tree: Option<&'a RetainedPointerTree>,
        ) -> Result<Guards<'a>, Refusal> {
            let pointers = self
                .0
                .registry
                .pointer_state
                .lock()
                .map_err(|_| Refusal::Unavailable)?;
            let authority = self
                .0
                .registry
                .input_authority
                .lock()
                .map_err(|_| Refusal::Unavailable)?;
            // A key retains the pointer's source tree separately when an
            // active grab delivers to another client. Match press lock order;
            // release must never acquire a lower client after the recipient.
            let (selections, pointer_selections) = if let Some(source) = pointer_tree {
                if source.client == client || Arc::ptr_eq(selection_owner, &source.selections) {
                    return Err(Refusal::ForeignOrigin);
                }
                let (selected, source_selected) = if client.raw() < source.client.raw() {
                    let selected = selection_owner
                        .lock()
                        .map_err(|_| Refusal::SelectionUnavailable)?;
                    let source_selected = source
                        .selections
                        .lock()
                        .map_err(|_| Refusal::SelectionUnavailable)?;
                    (selected, source_selected)
                } else {
                    let source_selected = source
                        .selections
                        .lock()
                        .map_err(|_| Refusal::SelectionUnavailable)?;
                    let selected = selection_owner
                        .lock()
                        .map_err(|_| Refusal::SelectionUnavailable)?;
                    (selected, source_selected)
                };
                if source_selected.private_origin
                    != Some(PrivateAppliedSelectionOrigin {
                        authority: self.0.identity,
                        namespace: self.0.namespace,
                        client: source.client,
                    })
                {
                    return Err(Refusal::ForeignOrigin);
                }
                (selected, Some(source_selected))
            } else {
                (
                    selection_owner
                        .lock()
                        .map_err(|_| Refusal::SelectionUnavailable)?,
                    None,
                )
            };
            if selections.private_origin
                != Some(PrivateAppliedSelectionOrigin {
                    authority: self.0.identity,
                    namespace: self.0.namespace,
                    client,
                })
            {
                return Err(Refusal::ForeignOrigin);
            }
            Ok(Guards {
                origin: &self.0,
                pointers,
                authority,
                selections,
                selection_owner,
                pointer_selections,
                pointer_tree,
                client,
                generation,
            })
        }
    }

    /// The full obligation is installed in caller-owned storage before the
    /// common press. Neither a native unwind nor a release refusal can erase
    /// its origin, its exact connection state, or how far it progressed.
    pub(super) struct Hold {
        origin: Arc<Origin>,
        selections: Arc<Mutex<XCoreEventSelectionState>>,
        client: XServerFrontendClientId,
        generation: u64,
        /// Exactly which endpoint this hold's events are owed to.
        ///
        /// Taken once, when the obligation is installed, from the records that
        /// were held and cross-checked at that moment. A release or a join
        /// keeps THIS one and never refreshes it from whatever is bound now:
        /// the events belong to the endpoint that was there when the button
        /// went down, and a refreshed identity would let a replacement
        /// registration inherit an emission it never asked for.
        endpoint: PrivateEndpointIdentity,
        query_scope: Option<crate::OrderedQueryScopeReceipt>,
        input: Input,
        incarnation: Option<HoldIncarnation>,
        grant: GrantId,
        evdev: u32,
        button: u8,
        surface_window: XResourceId,
        pointer_window: XResourceId,
        query_surface_window: XResourceId,
        plan: PrivateResolvedPointer,
        press_event: XAuthorityPointerEvent,
        press_emission: Option<PrivateOrderedEmission>,
        release_emission: Option<PrivateOrderedEmission>,
        surface: SurfaceId,
        activation: Option<crate::PointerActivationCommit>,
        route_lease: Option<sophia_protocol::ApplicationRouteLeaseIdentity>,
        grab_lease: Option<sophia_protocol::ApplicationRouteLeaseIdentity>,
        status: Status,
        proof: Option<Proof>,
        activation_retirement: Option<ActivationRetirement>,
        release_mapper_applied: bool,
    }

    /// Source evidence for one automatic activation, retained by the hold
    /// whose native release retired it. This says nothing about that hold's
    /// query/selection cleanup, common debt, or recipient delivery.
    pub(super) struct ActivationRetirement {
        origin: Arc<Origin>,
        stamp: crate::PointerActivationStamp,
    }

    /// A clone of the exact retained connection capability, not a route lookup.
    /// This separate borrow lets cleanup mutate its phase while guards borrow
    /// the same connection's storage; cloning it allocates nothing.
    pub(super) struct RetainedConnection {
        origin: Arc<Origin>,
        selections: Arc<Mutex<XCoreEventSelectionState>>,
        client: XServerFrontendClientId,
        generation: u64,
        /// Carried unchanged from the hold, for the same reason the hold keeps
        /// it: cleanup answers for the endpoint the work was accepted under.
        endpoint: PrivateEndpointIdentity,
        pointer_tree: Option<RetainedPointerTree>,
    }

    impl RetainedConnection {
        /// Exactly which endpoint this connection is, as it was when the
        /// obligation was installed.
        pub(super) fn endpoint(&self) -> &PrivateEndpointIdentity {
            &self.endpoint
        }

        /// STAGE-ONLY SEAM, TEST BUILDS ONLY: re-address this retained
        /// connection to another admitted endpoint. A control uses it to
        /// supply a resolved capsule to a real connection while the private
        /// producer that would address one is not yet attached; production
        /// builds compile no such re-addressing.
        #[cfg(all(test, unix))]
        pub(super) fn readdressed(
            mut self,
            client: XServerFrontendClientId,
            generation: u64,
            endpoint: PrivateEndpointIdentity,
        ) -> Self {
            self.client = client;
            self.generation = generation;
            self.endpoint = endpoint;
            self
        }
    }

    #[derive(Clone)]
    struct RetainedPointerTree {
        selections: Arc<Mutex<XCoreEventSelectionState>>,
        client: XServerFrontendClientId,
    }

    impl Hold {
        pub(super) fn connection(&self) -> RetainedConnection {
            RetainedConnection {
                origin: self.origin.clone(),
                selections: self.selections.clone(),
                client: self.client,
                generation: self.generation,
                endpoint: self.endpoint.clone(),
                pointer_tree: None,
            }
        }
        pub(super) fn plan(&self) -> PrivateResolvedPointer {
            self.plan
        }
        pub(super) fn press_event(&self) -> XAuthorityPointerEvent {
            self.press_event
        }
        /// The input this hold is named against.
        ///
        /// Read so a caller can select the hold a join belongs to without
        /// resolving anything: the hold already knows which input it is for.
        pub(super) fn input(&self) -> Input {
            self.input
        }

        /// The recipient this hold's press actually reached.
        ///
        /// A join delivers where the press went. Resolving a recipient again
        /// would answer a different question -- where the route would reach
        /// now -- and bind this delivery to whoever that is.
        pub(super) fn client(&self) -> XServerFrontendClientId {
            self.client
        }

        pub(super) fn status(&self) -> Status {
            self.status
        }
        pub(super) fn incarnation(&self) -> Option<HoldIncarnation> {
            self.incarnation
        }
        pub(super) fn proof(&self) -> Option<&Proof> {
            self.proof.as_ref()
        }

        /// The terminal owner must retain this hold while siblings still need
        /// its evidence. No global history or caller-created receipt exists.
        pub(super) fn activation_retirement(&self) -> Option<&ActivationRetirement> {
            self.activation_retirement.as_ref()
        }

        /// This residual is recorded only after this hold's other native
        /// contributions were cleared. Combine that fact with source-produced
        /// retirement of the exact shared activation; do not replay a release
        /// or infer retirement from whichever grab happens to exist now.
        pub(super) fn complete_shared_activation(
            &mut self,
            retirement: &ActivationRetirement,
        ) -> Result<&Proof, Refusal> {
            if self.status
                != Status::Retained(Residual::Activation(
                    crate::PointerActivationRetirement::StillRequiredByOtherButtons,
                ))
            {
                return Err(Refusal::WrongPhase);
            }
            if !Arc::ptr_eq(&self.origin, &retirement.origin) {
                return Err(Refusal::ForeignOrigin);
            }
            if self.activation.is_none_or(|activation| {
                !activation.automatic() || activation.stamp() != retirement.stamp
            }) {
                return Err(Refusal::ActivationMismatch);
            }
            let incarnation = self.incarnation.ok_or(Refusal::WrongPhase)?;
            self.proof = Some(Proof {
                origin: self.origin.clone(),
                incarnation,
                grant: self.grant,
            });
            self.status = Status::NativeReconciled;
            Ok(self.proof.as_ref().expect("source installed proof"))
        }
    }

    /// Evidence only for the native obligation of this exact incarnation.
    /// It contains no transport receipt and cannot set recipient_settled.
    pub(super) struct Proof {
        origin: Arc<Origin>,
        incarnation: HoldIncarnation,
        grant: GrantId,
    }
    impl Proof {
        pub(super) fn incarnation(&self) -> HoldIncarnation {
            self.incarnation
        }

        /// Called with no adapter guards held. The proof supplies its issuer,
        /// participant and full incarnation; callers cannot substitute any of
        /// them. `false` does not mean absent: settle may record the native bit
        /// while the independent recipient obligation remains outstanding.
        pub(super) fn record_native(&self) -> Result<bool, PrivateAuthorityRefusal> {
            self.origin
                .controller
                .under_common_as_origin(|authority, issuer| {
                    authority
                        .settle(
                            issuer,
                            Some(self.grant),
                            self.incarnation.input,
                            self.incarnation,
                            SettlementBit {
                                native_reconciled: true,
                                recipient_settled: false,
                            },
                        )
                        .map_err(PrivateAuthorityRefusal::Authority)
                })?
        }
    }

    pub(super) struct BaseGuards<'a> {
        origin: &'a Arc<Origin>,
        pointers: MutexGuard<'a, BTreeMap<(NamespaceId, SeatId), crate::XCorePointerMapper>>,
        authority: MutexGuard<'a, crate::XInputAuthorityState>,
    }

    impl BaseGuards<'_> {
        /// New aggregate press. Resolution reads these exact selections and
        /// the exclusive prepared grab. The source then binds the original
        /// delivery through its own origin, before the common or native effect.
        /// Neither callback may reacquire these guards or common. The caller
        /// keeps its execution claim across this operation; binding does not
        /// replace cancellation arbitration. A join uses `join` below and
        /// never resolves a new recipient.
        #[allow(clippy::too_many_arguments)]
        pub(super) fn press<'connection>(
            &mut self,
            permit: &mut ExecutionPermit<'_>,
            capability: DeviceCapability,
            route: &XAuthorityRoutedInput,
            surface_window: XResourceId,
            implicit: crate::XActiveInputGrab,
            storage: &mut Option<Hold>,
            may_have_applied: &Cell<bool>,
            select_client: impl FnOnce(
                XServerFrontendClientId,
            ) -> Result<
                PrivateAppliedClientRef<'connection>,
                PrivateAppliedRegistryRefusal,
            >,
            resolve: impl FnOnce(
                &PrivateAppliedClientRef<'connection>,
                &XCoreEventSelectionState,
                &crate::PreparedPointerPress<'_>,
                &XAuthorityPointerEvent,
            ) -> Result<PrivateResolvedPointer, PrivateAppliedRefusal>,
        ) -> Result<(Applied, Option<XAuthorityPointerEvent>), Refusal> {
            if permit.identity() != self.origin.identity {
                return Err(Refusal::ForeignOrigin);
            }
            if capability.source() != permit.source() {
                return Err(Refusal::ForeignOrigin);
            }
            if storage.is_some() {
                return Err(Refusal::WrongPhase);
            }
            if route.request.seat != self.origin.seat {
                return Err(Refusal::ForeignOrigin);
            }
            let InputEventKind::PointerButton {
                button: evdev,
                pressed: true,
            } = route.request.kind
            else {
                return Err(Refusal::InvalidButton);
            };
            let button = crate::XCorePointerMapper::peek_evdev_button(evdev)
                .ok_or(Refusal::InvalidButton)?;
            let input = Input::button(
                button,
                sophia_input_authority::Capacity::PLANNED.button_domain(),
            )
            .map_err(|_| Refusal::InvalidButton)?;
            let pointer = self
                .pointers
                .get_mut(&(self.origin.namespace, self.origin.seat))
                .ok_or(Refusal::MissingMapper)?;
            if !self.authority.has_ordered_namespace(self.origin.namespace) {
                return Err(Refusal::MissingQueryScope);
            }
            let modifiers = self
                .authority
                .pointer_query_state(self.origin.namespace)
                .mask
                & 0xff;
            let mut event = pointer_event(route, button, true, modifiers | pointer.state());
            let query_scope = self.authority.ordered_query_scope(self.origin.namespace);
            let prepared = self
                .authority
                .prepare_pointer_press(self.origin.namespace, button, modifiers, implicit)
                .map_err(Refusal::Preparation)?;
            let recipient = XServerFrontendClientId::from_raw(prepared.recipient().owner);
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
            if selections.private_origin
                != Some(PrivateAppliedSelectionOrigin {
                    authority: self.origin.identity,
                    namespace: self.origin.namespace,
                    client: recipient,
                })
            {
                return Err(Refusal::ForeignOrigin);
            }
            if selections
                .applied_revision
                .and_then(|n| n.checked_add(1))
                .is_none()
            {
                return Err(Refusal::SelectionUnavailable);
            }
            let plan =
                resolve(&client, &selections, &prepared, &event).map_err(Refusal::Resolution)?;
            let delivered_window = plan
                .primary_recipient_window()
                .map_err(Refusal::Resolution)?;
            let pointer_window = plan.event_window;
            // A cross-client grab may resolve in root coordinates because the
            // recipient does not own the Engine surface's geometry. Keep that
            // selected basis distinct from the original shared query anchor.
            let selected_coordinates = selections
                .ordered_coordinates_budget(
                    XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                    plan.surface_window,
                    event.root_x,
                    event.root_y,
                    &PrivateTraversalBudget::new(),
                )
                .map_err(Refusal::Resolution)?;
            let mut selected_event = event;
            selected_event.event_x = selected_coordinates.0;
            selected_event.event_y = selected_coordinates.1;
            let prepared = if prepared.is_new_implicit() {
                prepared
                    .with_implicit_window(delivered_window)
                    .map_err(|_| Refusal::WrongRecipient)?
            } else {
                prepared
            };
            let grab_lease = prepared.recipient().route_lease;
            // Recovery ranks below the held X guards. Its cancellation and
            // disconnect paths release the ledger before taking X authority.
            // Do not delegate this to resolution: a delivery refusal is not a
            // selection failure, and checking it after press leaves a hidden
            // common hold for a recipient that could never accept the press.
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
            *storage = Some(Hold {
                origin: self.origin.clone(),
                selections: client.connection.selections.clone(),
                client: client.client,
                generation: client._admission.generation,
                endpoint: client.endpoint.clone(),
                query_scope,
                input,
                incarnation: None,
                grant: capability.grant(),
                evdev,
                button,
                surface_window: plan.surface_window,
                pointer_window,
                query_surface_window: surface_window,
                plan,
                press_event: selected_event,
                press_emission: None,
                release_emission: None,
                surface: event.surface,
                activation: None,
                route_lease: route.route_lease,
                grab_lease,
                status: Status::PressEntered,
                proof: None,
                activation_retirement: None,
                release_mapper_applied: false,
            });
            may_have_applied.set(true);
            let applied = permit
                .press(
                    input,
                    Recipient {
                        recipient: client.client.raw(),
                        connection_generation: client._admission.generation,
                    },
                )
                .map_err(Refusal::Authority)?;
            let hold = storage
                .as_mut()
                .expect("source installed context before effect");
            hold.incarnation = Some(applied.incarnation());
            if !applied.first_press() {
                // No native mutation. A caller that proposed a new aggregate
                // must retain/discriminate this disagreement against its old
                // context; this one cannot prove the old native obligation.
                hold.status = Status::Retained(Residual::IncarnationMismatch);
                return Ok((applied, None));
            }
            hold.activation = Some(prepared.commit_stamped());
            let (_, before) = pointer
                .map_evdev_button(evdev, true)
                .expect("validated button");
            event.state = modifiers | before;
            self.authority.observe_query_input(
                self.origin.namespace,
                surface_window,
                XAuthorityInputEvent::Pointer(event),
            );
            selections.observe_pointer(
                plan.surface_window,
                pointer_window,
                event.root_x,
                event.root_y,
                selected_event.event_x,
                selected_event.event_y,
                modifiers | pointer.state(),
            );
            selected_event.state = event.state;
            hold.press_event = selected_event;
            hold.status = Status::Held;
            hold.press_emission = Some(PrivateOrderedEmission::pointer(
                hold,
                route.delivery,
                selected_event,
                plan,
            ));
            Ok((applied, Some(selected_event)))
        }
    }

    pub(super) struct Guards<'a> {
        origin: &'a Arc<Origin>,
        pointers: MutexGuard<'a, BTreeMap<(NamespaceId, SeatId), crate::XCorePointerMapper>>,
        authority: MutexGuard<'a, crate::XInputAuthorityState>,
        selections: MutexGuard<'a, XCoreEventSelectionState>,
        selection_owner: &'a Arc<Mutex<XCoreEventSelectionState>>,
        pointer_selections: Option<MutexGuard<'a, XCoreEventSelectionState>>,
        pointer_tree: Option<&'a RetainedPointerTree>,
        client: XServerFrontendClientId,
        generation: u64,
    }

    impl Guards<'_> {
        fn validate_hold(&self, hold: &Hold) -> Result<(), Refusal> {
            if !Arc::ptr_eq(self.origin, &hold.origin)
                || !Arc::ptr_eq(self.selection_owner, &hold.selections)
                || self.client != hold.client
                || self.generation != hold.generation
            {
                return Err(Refusal::ForeignOrigin);
            }
            Ok(())
        }

        fn validate(&self, permit: &ExecutionPermit<'_>) -> Result<(), Refusal> {
            if permit.identity() != self.origin.identity {
                return Err(Refusal::ForeignOrigin);
            }
            Ok(())
        }

        pub(super) fn join(
            &mut self,
            permit: &mut ExecutionPermit<'_>,
            hold: &Hold,
            may_have_applied: &Cell<bool>,
        ) -> Result<Applied, Refusal> {
            self.validate(permit)?;
            self.validate_hold(hold)?;
            if hold.status != Status::Held {
                return Err(Refusal::WrongPhase);
            }
            may_have_applied.set(true);
            let applied = permit
                .press(
                    hold.input,
                    Recipient {
                        recipient: hold.client.raw(),
                        connection_generation: hold.generation,
                    },
                )
                .map_err(Refusal::Authority)?;
            if applied.first_press() || Some(applied.incarnation()) != hold.incarnation {
                return Err(Refusal::WrongRecipient);
            }
            Ok(applied)
        }

        /// The common aggregate release and every native projection occur in
        /// the same interval. Final release keeps an exact residual even when
        /// native state is missing; it never recreates a mapper or namespace.
        pub(super) fn release(
            &mut self,
            permit: &mut ExecutionPermit<'_>,
            hold: &mut Hold,
            route: &XAuthorityRoutedInput,
            may_have_applied: &Cell<bool>,
        ) -> Result<
            (
                ReleaseOutcome,
                Result<Option<XAuthorityPointerEvent>, PrivateAppliedRefusal>,
            ),
            Refusal,
        > {
            self.validate(permit)?;
            self.validate_hold(hold)?;
            if hold.status != Status::Held {
                return Err(Refusal::WrongPhase);
            }
            let InputEventKind::PointerButton {
                button,
                pressed: false,
            } = route.request.kind
            else {
                return Err(Refusal::InvalidButton);
            };
            if button != hold.evdev {
                return Err(Refusal::InvalidButton);
            }
            hold.status = Status::ReleaseEntered;
            may_have_applied.set(true);
            let outcome = permit.release(hold.input).map_err(Refusal::Authority)?;
            let ReleaseOutcome::DeliverTo(incarnation) = outcome else {
                hold.status = Status::Held;
                return Ok((outcome, Ok(None)));
            };
            if Some(incarnation) != hold.incarnation || incarnation.input != hold.input {
                hold.status = Status::Retained(Residual::IncarnationMismatch);
                return Ok((outcome, Err(PrivateAppliedRefusal::Interrupted)));
            }
            let Some(pointer) = self
                .pointers
                .get_mut(&(self.origin.namespace, self.origin.seat))
            else {
                hold.status = Status::Retained(Residual::MissingMapper);
                return Ok((outcome, Err(PrivateAppliedRefusal::Interrupted)));
            };
            // Mapper cleanup is still owed if query or selected projection
            // becomes unavailable; neither can turn a partial result clean.
            let query_present = self.authority.has_ordered_namespace(self.origin.namespace);
            let modifiers = if query_present {
                self.authority
                    .pointer_query_state(self.origin.namespace)
                    .mask
                    & 0xff
            } else {
                0
            };
            let (_, before) = pointer
                .map_evdev_button(hold.evdev, false)
                .expect("press validated button");
            hold.release_mapper_applied = true;
            let mut event = pointer_event(route, hold.button, false, modifiers | before);
            event.surface = hold.surface;
            // Root coordinates belong to this accepted release; route-local
            // coordinates may describe a different surface by now. Convert
            // under the exact retained connection's held geometry without
            // selecting a target. Failure retains an owed, unbuilt event.
            let coordinates = self.selections.ordered_coordinates_budget(
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                hold.surface_window,
                event.root_x,
                event.root_y,
                &PrivateTraversalBudget::new(),
            );
            let activation = hold
                .activation
                .expect("held phase follows recorded activation");
            let retirement = if activation.automatic() {
                self.authority.retire_pointer_activation(
                    self.origin.namespace,
                    activation.stamp(),
                    hold.button,
                    pointer,
                )
            } else {
                crate::PointerActivationRetirement::Explicit
            };
            if retirement == crate::PointerActivationRetirement::Retired {
                // Record before later independent cleanup. A sibling may use
                // this exact fact even if this hold still owes another native
                // contribution; it does not settle that separate obligation.
                hold.activation_retirement = Some(ActivationRetirement {
                    origin: hold.origin.clone(),
                    stamp: activation.stamp(),
                });
            }
            let query_cleared = self
                .authority
                .observe_query_button_release(self.origin.namespace, hold.button)
                .is_ok();
            // Update only the contribution that this aggregate release ends.
            // The exact connection may have observed motion/modifiers since
            // the press. Restoring an old position or today's route would
            // replace another operation's applied state with this debt.
            let selected_present = self.selections.pointer.is_some();
            let revision = self.selections.begin_applied_mutation();
            if let Some(observation) = self.selections.pointer.as_mut()
                && (1..=5).contains(&hold.button)
            {
                observation.mask &= !(1 << (hold.button + 7));
            }
            self.selections.finish_applied_mutation(revision);
            let residual = if !query_cleared {
                Some(Residual::MissingQueryScope)
            } else if !selected_present || self.selections.applied_revision.is_none() {
                Some(Residual::SelectionUnavailable)
            } else if hold.route_lease.is_some() || hold.grab_lease.is_some() {
                Some(Residual::ExternalLease)
            } else if activation.recipient().pointer_mode == 0
                || activation.recipient().keyboard_mode == 0
            {
                Some(Residual::Synchronous)
            } else if activation.automatic()
                && retirement != crate::PointerActivationRetirement::Retired
            {
                Some(Residual::Activation(retirement))
            } else {
                None
            };
            if let Some(residual) = residual {
                hold.status = Status::Retained(residual);
            } else {
                hold.proof = Some(Proof {
                    origin: hold.origin.clone(),
                    incarnation,
                    grant: hold.grant,
                });
                hold.status = Status::NativeReconciled;
            }
            let event = if query_present {
                coordinates.map(|(x, y)| {
                    event.event_x = x;
                    event.event_y = y;
                    Some(event)
                })
            } else {
                Err(PrivateAppliedRefusal::Interrupted)
            };
            let event = event.and_then(|event| {
                let Some(event) = event else { return Ok(None) };
                let plan = self.release_plan(hold, event)?;
                hold.release_emission = Some(PrivateOrderedEmission::pointer(
                    hold,
                    route.delivery,
                    event,
                    plan,
                ));
                Ok(Some(event))
            });
            Ok((outcome, event))
        }
    }

    include!("private_native_emission.rs");
    include!("private_native_key_emission.rs");
    include!("private_native_keyboard.rs");
    include!("private_native_transient.rs");
    include!("private_native_reconcile.rs");

    fn pointer_event(
        route: &XAuthorityRoutedInput,
        button: u8,
        pressed: bool,
        state: u16,
    ) -> XAuthorityPointerEvent {
        XAuthorityPointerEvent {
            kind: XAuthorityPointerEventKind::Button { button, pressed },
            surface: route.request.target_surface,
            root_x: clamp_input_coordinate(route.request.global_position.x),
            root_y: clamp_input_coordinate(route.request.global_position.y),
            event_x: clamp_input_coordinate(route.request.local_position.x),
            event_y: clamp_input_coordinate(route.request.local_position.y),
            state,
            time_msec: u32::try_from(route.request.time_msec).unwrap_or(u32::MAX),
        }
    }
}
