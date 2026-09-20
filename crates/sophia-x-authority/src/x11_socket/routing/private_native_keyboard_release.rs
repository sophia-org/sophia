impl Guards<'_> {
    fn validate_key_hold(&self, hold: &KeyHold) -> Result<(), Refusal> {
        if !Arc::ptr_eq(self.origin, &hold.origin)
            || !Arc::ptr_eq(self.selection_owner, &hold.selections)
            || self.client != hold.client
            || self.generation != hold.generation
        {
            return Err(Refusal::ForeignOrigin);
        }
        match (&hold.pointer_tree, self.pointer_tree) {
            (None, None) => {}
            (Some(held), Some(locked))
                if held.client == locked.client
                    && Arc::ptr_eq(&held.selections, &locked.selections) => {}
            _ => return Err(Refusal::ForeignOrigin),
        }
        if hold.status != Status::Held {
            return Err(Refusal::WrongPhase);
        }
        Ok(())
    }

    /// The caller binds this delivery to the inherited recipient first. No
    /// focus, selected window or grab policy is read and XKB is not updated.
    pub(super) fn join_key(
        &mut self,
        permit: &mut ExecutionPermit<'_>,
        hold: &KeyHold,
        keyboards: &mut PrivateKeyboards,
        may_have_applied: &Cell<bool>,
    ) -> Result<Applied, Refusal> {
        self.validate(permit)?;
        self.validate_key_hold(hold)?;
        let keyboard = key_state(self.origin, keyboards)?;
        if keyboard.physical_key_state(hold.key) != crate::XkbPhysicalKeyState::Held {
            return Err(Refusal::KeyboardUnavailable);
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

    /// Final common release precedes XKB/native cleanup. An unavailable native
    /// history remains an owned residual; it cannot turn an ended aggregate
    /// back into a pre-effect refusal. Record any proof only after common drops.
    pub(super) fn release_key(
        &mut self,
        permit: &mut ExecutionPermit<'_>,
        hold: &mut KeyHold,
        route: &XAuthorityRoutedInput,
        keyboards: &mut PrivateKeyboards,
        may_have_applied: &Cell<bool>,
    ) -> Result<
        (
            ReleaseOutcome,
            Result<Option<XAuthorityKeyEvent>, PrivateAppliedRefusal>,
        ),
        Refusal,
    > {
        self.validate(permit)?;
        self.validate_key_hold(hold)?;
        if !keyboards.answers_for(self.origin.identity) || route.request.seat != self.origin.seat {
            return Err(Refusal::ForeignOrigin);
        }
        if !matches!(route.request.kind, InputEventKind::Key { keycode, pressed: false } if keycode == hold.evdev)
        {
            return Err(Refusal::InvalidKey);
        }
        // Captured from the exact surviving native activation under this
        // same guard, before a passive trigger release may retire it. Source
        // absence later is never substituted for its actual Async receipt.
        let thaw = hold.activation.and_then(|activation| {
            self.authority.ordered_keyboard_thaw(self.origin.namespace, activation.stamp())
        });
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
        hold.release_disposition = if route.mode == XAuthorityRoutedInputMode::StateOnly {
            KeyReleaseDisposition::RecipientTerminationRequired
        } else {
            KeyReleaseDisposition::Deliver
        };
        let built = self.finish_key_release(
            hold,
            keyboards,
            incarnation,
            thaw,
            u32::try_from(route.request.time_msec).unwrap_or(u32::MAX),
            route.delivery,
        );
        Ok((outcome, built))
    }

    /// Finish a release the ledger already made, because the source that was
    /// holding the key departed.
    ///
    /// NOT A RELEASE THIS DECIDES. `revoke_grant` retires a departing source
    /// and releases every input it held, and when that ends the aggregate the
    /// ledger has already decided everything a release decides. What it
    /// cannot do from there is tell the keyboard, or build the event the
    /// recipient is owed, because neither is the ledger's to reach. This is
    /// that half, and it asks the ledger for nothing.
    ///
    /// The permit is the reconciliation one rather than an execution permit,
    /// which is the honest kind: there is no request being executed, the
    /// incarnation comes from the permit rather than from a return value, and
    /// the authority has already applied the only thing it was going to.
    ///
    /// The time is the authority's own last observation rather than a
    /// request's, because the request that would have carried one does not
    /// exist. That is the same clock every other event on this connection was
    /// stamped from, so the release does not travel backwards past the press
    /// it ends.
    ///
    /// The inner result is the event, or why it could not be built, which is
    /// the same pair a requested release answers with and is kept apart for
    /// the same reason: a release owing an event nobody could build is not a
    /// release owing none.
    pub(super) fn retire_key_release(
        &mut self,
        permit: &sophia_input_authority::NativeReconciliationPermit<'_>,
        hold: &mut KeyHold,
        keyboards: &mut PrivateKeyboards,
        may_have_applied: &Cell<bool>,
    ) -> Result<Result<Option<XAuthorityKeyEvent>, PrivateAppliedRefusal>, Refusal> {
        if permit.identity() != self.origin.identity || permit.owner() != Some(hold.grant) {
            return Err(Refusal::ForeignOrigin);
        }
        self.validate_key_hold(hold)?;
        if !keyboards.answers_for(self.origin.identity) {
            return Err(Refusal::ForeignOrigin);
        }
        let incarnation = permit.incarnation();
        if Some(incarnation) != hold.incarnation || incarnation.input != hold.input {
            return Err(Refusal::WrongRecipient);
        }
        // Read before anything native is touched, for the reason the request
        // path reads it before its ledger call: a passive trigger release may
        // retire the activation, and the receipt that answers it belongs to
        // the activation as it stands now.
        let thaw = hold
            .activation
            .and_then(|activation| {
                self.authority
                    .ordered_keyboard_thaw(self.origin.namespace, activation.stamp())
            });
        let time_msec = self
            .authority
            .pointer_query_state(self.origin.namespace)
            .last_time_msec;
        hold.status = Status::ReleaseEntered;
        hold.release_disposition = KeyReleaseDisposition::Deliver;
        may_have_applied.set(true);
        Ok(self.finish_key_release(hold, keyboards, incarnation, thaw, time_msec, None))
    }

    /// Everything a release owes after the ledger has ended the aggregate.
    ///
    /// SEPARATED BECAUSE THE LEDGER STEP IS NOT ALWAYS HERE. A release a
    /// request asked for ends the aggregate through its own execution permit,
    /// immediately above this. A release a departing source caused ended it
    /// inside `revoke_grant`, before anything native was told, and calling the
    /// ledger again there would release one thing twice. What both owe from
    /// that point is identical -- the key up in the keyboard's own history,
    /// the modifiers as they now stand, the activation retired if this was
    /// the last thing holding it, the residual when some part of that could
    /// not be established, and the emission the terminal will carry to the
    /// recipient -- so it is written once.
    ///
    /// `delivery` is an `Option` because a release nobody asked for names no
    /// delivery, and an emission has always been able to carry that.
    fn finish_key_release(
        &mut self,
        hold: &mut KeyHold,
        keyboards: &mut PrivateKeyboards,
        incarnation: HoldIncarnation,
        thaw: Option<crate::OrderedKeyboardThaw>,
        time_msec: u32,
        delivery: Option<XAuthorityInputDeliveryId>,
    ) -> Result<Option<XAuthorityKeyEvent>, PrivateAppliedRefusal> {
        let keyboard = match key_state(self.origin, keyboards) {
            Ok(state) if state.physical_key_state(hold.key) == crate::XkbPhysicalKeyState::Held => {
                state
            }
            _ => {
                hold.status = Status::Retained(Residual::KeyboardUnavailable);
                return Err(PrivateAppliedRefusal::Interrupted);
            }
        };
        let before = keyboard.ordered_state().expect("known held source key");
        let (_, modifiers) = keyboard
            .map_evdev_key(hold.evdev, false)
            .expect("retained valid key");
        hold.release_xkb_applied = true;
        let Some(after) = keyboard.ordered_state() else {
            hold.status = Status::Retained(Residual::KeyboardUnavailable);
            return Err(PrivateAppliedRefusal::Interrupted);
        };
        let query_present = self.authority.has_ordered_namespace(self.origin.namespace);
        let position = self
            .authority
            .pointer_query_state(self.origin.namespace)
            .position;
        let pointer = self
            .pointers
            .get(&(self.origin.namespace, self.origin.seat));
        let buttons = pointer.map(|pointer| pointer.state());
        let event = XAuthorityKeyEvent {
            keycode: hold.key,
            pressed: false,
            state: modifiers | buttons.unwrap_or(0),
            modifiers_after: keyboard.modifier_mask() as u8,
            time_msec,
        };
        // Never use observe_query_modifiers to recreate a missing namespace.
        if query_present {
            self.authority
                .observe_query_modifiers(self.origin.namespace, u16::from(event.modifiers_after));
        }
        observe_key_modifiers(&mut self.selections, event.modifiers_after);
        let retirement = hold
            .activation
            .filter(|activation| activation.trigger().is_some())
            .map(|activation| {
                self.authority.retire_keyboard_activation(
                    self.origin.namespace,
                    activation.stamp(),
                    hold.key,
                    keyboard,
                )
            });
        if retirement == Some(crate::KeyboardActivationRetirement::Retired) {
            hold.activation_retirement = Some(KeyActivationRetirement {
                origin: hold.origin.clone(),
                stamp: hold.activation.expect("retired activation").stamp(),
            });
        }
        let residual = if !query_present {
            Some(Residual::MissingQueryScope)
        } else if buttons.is_none() {
            Some(Residual::MissingMapper)
        } else if self.selections.applied_revision.is_none() {
            Some(Residual::SelectionUnavailable)
        } else if hold.route_lease.is_some()
            || hold
                .activation
                .is_some_and(|activation| activation.recipient().route_lease.is_some())
        {
            Some(Residual::ExternalLease)
        } else if hold.activation.is_some_and(|activation| {
            (activation.recipient().pointer_mode == 0 || activation.recipient().keyboard_mode == 0)
                && thaw.as_ref().is_none_or(|receipt| !receipt.answers(activation.stamp()))
        }) {
            Some(Residual::Synchronous)
        } else {
            retirement
                .filter(|outcome| *outcome != crate::KeyboardActivationRetirement::Retired)
                .map(Residual::KeyboardActivation)
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
        if hold.release_disposition == KeyReleaseDisposition::RecipientTerminationRequired {
            // Common and the original XKB history have released the key. No
            // protocol key or StateNotify is constructed; the original
            // recipient still requires its own exact termination evidence.
            return Ok(None);
        }
        (|| {
            if !query_present || buttons.is_none() {
                return Err(PrivateAppliedRefusal::Interrupted);
            }
            let position = position.ok_or(PrivateAppliedRefusal::Interrupted)?;
            let pointer_path = key_pointer_path(
                &self.selections,
                self.pointer_selections
                    .as_deref()
                    .unwrap_or(&self.selections),
                position,
            )
            .map_err(|cause| match cause {
                crate::KeyboardPreparationRefusal::Applied(cause) => cause,
                _ => PrivateAppliedRefusal::Interrupted,
            })?;
            let mut plan = hold.plan;
            // Keep the delivery window/forms, but the child is a property of
            // this release's pointer relation, never a copied press observation.
            let depth = pointer_path.depth(plan.target.window);
            plan.target.child = depth
                .and_then(|depth| depth.checked_sub(1))
                .map_or(XResourceId::NONE, |index| pointer_path.as_slice()[index]);
            plan.target.ancestry_depth = depth.unwrap_or(0);
            u32::try_from(plan.target.child.local.raw())
                .map_err(|_| PrivateAppliedRefusal::WireResourceOverflow)?;
            let (x, y) = self.selections.ordered_coordinates_budget(
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                plan.target.window,
                position.root_x,
                position.root_y,
                &PrivateTraversalBudget::new(),
            )?;
            plan.target.event_x = x;
            plan.target.event_y = y;
            // This notification selection is read for this release while its
            // exact connection is held; the key's target/forms stay inherited.
            plan.xkb_details = self.selections.xkb_state_details;
            let payload = KeyEmission::new(event, plan, position, before, after)?;
            hold.release_emission = Some(PrivateOrderedEmission::key(hold, delivery, payload));
            Ok(Some(event))
        })()
    }

}
