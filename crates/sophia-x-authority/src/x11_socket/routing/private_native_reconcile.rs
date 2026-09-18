// Native cleanup after issuer revocation. No new input execution, route or
// event is fabricated. The common debt remains distinct from its recipient.

impl Guards<'_> {
    fn validate_retired(
        &self,
        permit: &sophia_input_authority::NativeReconciliationPermit<'_>,
        origin: &Arc<Origin>,
        incarnation: Option<HoldIncarnation>,
        grant: GrantId,
        endpoint: &PrivateEndpointIdentity,
        selections: &Arc<Mutex<XCoreEventSelectionState>>,
    ) -> Result<(), Refusal> {
        if !Arc::ptr_eq(self.origin, origin)
            || permit.identity() != self.origin.identity
            || Some(permit.incarnation()) != incarnation
            || permit.owner() != Some(grant)
            || !Arc::ptr_eq(self.selection_owner, selections)
            || self.client != endpoint.client
            || self.generation != endpoint.generation
        {
            return Err(Refusal::ForeignOrigin);
        }
        Ok(())
    }

    pub(super) fn reconcile_pointer(
        &mut self,
        permit: &sophia_input_authority::NativeReconciliationPermit<'_>,
        hold: &mut Hold,
    ) -> Result<bool, Refusal> {
        self.validate_retired(
            permit,
            &hold.origin,
            hold.incarnation,
            hold.grant,
            &hold.endpoint,
            &hold.selections,
        )?;
        if hold.status == Status::NativeReconciled {
            return Ok(false);
        }
        if matches!(hold.status, Status::PressEntered | Status::ReleaseEntered) {
            return Err(Refusal::WrongPhase);
        }
        if !hold
            .query_scope
            .as_ref()
            .is_some_and(crate::OrderedQueryScopeReceipt::retired)
        {
            return Err(Refusal::MissingQueryScope);
        }
        let cleanup = hold
            .endpoint
            .native_cleanup_receipt(&self.origin.registry.input_authority)
            .ok_or(Refusal::Unavailable)?;
        let activation = hold.activation.ok_or(Refusal::ActivationMismatch)?;
        if !cleanup.removed_pointer(activation.stamp()) {
            return Err(Refusal::ActivationMismatch);
        }
        if hold.route_lease.is_some() || hold.grab_lease.is_some() {
            hold.status = Status::Retained(Residual::ExternalLease);
            return Ok(false);
        }
        let pointer = self
            .pointers
            .get_mut(&(self.origin.namespace, self.origin.seat))
            .ok_or(Refusal::MissingMapper)?;
        if !hold.release_mapper_applied && !pointer.button_is_pressed(hold.button) {
            return Err(Refusal::WrongPhase);
        }
        if self.selections.pointer.is_none() || self.selections.applied_revision.is_none() {
            return Err(Refusal::SelectionUnavailable);
        }
        // Write ahead. An interrupted effect is retained, never replayed.
        hold.status = Status::ReleaseEntered;
        if !hold.release_mapper_applied {
            pointer
                .map_evdev_button(hold.evdev, false)
                .ok_or(Refusal::InvalidButton)?;
            hold.release_mapper_applied = true;
        }
        let revision = self.selections.begin_applied_mutation();
        if let Some(observation) = self.selections.pointer.as_mut()
            && (1..=5).contains(&hold.button)
        {
            observation.mask &= !(1 << (hold.button + 7));
        }
        self.selections.finish_applied_mutation(revision);
        hold.activation_retirement = Some(ActivationRetirement {
            origin: hold.origin.clone(),
            stamp: activation.stamp(),
        });
        hold.proof = Some(Proof {
            origin: hold.origin.clone(),
            incarnation: permit.incarnation(),
            grant: hold.grant,
        });
        hold.status = Status::NativeReconciled;
        Ok(true)
    }

    pub(super) fn reconcile_key(
        &mut self,
        permit: &sophia_input_authority::NativeReconciliationPermit<'_>,
        hold: &mut KeyHold,
        keyboards: &mut PrivateKeyboards,
    ) -> Result<bool, Refusal> {
        self.validate_retired(
            permit,
            &hold.origin,
            hold.incarnation,
            hold.grant,
            &hold.endpoint,
            &hold.selections,
        )?;
        if hold.status == Status::NativeReconciled {
            return Ok(false);
        }
        if matches!(hold.status, Status::PressEntered | Status::ReleaseEntered) {
            return Err(Refusal::WrongPhase);
        }
        if !hold
            .query_scope
            .as_ref()
            .is_some_and(crate::OrderedQueryScopeReceipt::retired)
        {
            return Err(Refusal::MissingQueryScope);
        }
        let cleanup = hold
            .endpoint
            .native_cleanup_receipt(&self.origin.registry.input_authority)
            .ok_or(Refusal::Unavailable)?;
        if let Some(activation) = hold.activation
            && !cleanup.removed_keyboard(activation.stamp())
        {
            return Err(Refusal::ActivationMismatch);
        }
        if hold.route_lease.is_some()
            || hold
                .activation
                .is_some_and(|activation| activation.recipient().route_lease.is_some())
        {
            hold.status = Status::Retained(Residual::ExternalLease);
            return Ok(false);
        }
        if self.selections.applied_revision.is_none() {
            return Err(Refusal::SelectionUnavailable);
        }
        let keyboard = key_state(self.origin, keyboards)?;
        let required = if hold.release_xkb_applied {
            crate::XkbPhysicalKeyState::Released
        } else {
            crate::XkbPhysicalKeyState::Held
        };
        if keyboard.physical_key_state(hold.key) != required {
            return Err(Refusal::KeyboardUnavailable);
        }
        hold.status = Status::ReleaseEntered;
        if !hold.release_xkb_applied {
            keyboard
                .map_evdev_key(hold.evdev, false)
                .ok_or(Refusal::KeyboardUnavailable)?;
            hold.release_xkb_applied = true;
        }
        observe_key_modifiers(&mut self.selections, keyboard.modifier_mask() as u8);
        if let Some(activation) = hold.activation {
            hold.activation_retirement = Some(KeyActivationRetirement {
                origin: hold.origin.clone(),
                stamp: activation.stamp(),
            });
        }
        hold.proof = Some(Proof {
            origin: hold.origin.clone(),
            incarnation: permit.incarnation(),
            grant: hold.grant,
        });
        hold.status = Status::NativeReconciled;
        Ok(true)
    }
}

impl Hold {
    pub(super) fn endpoint(&self) -> &PrivateEndpointIdentity {
        &self.endpoint
    }
    pub(super) fn grant(&self) -> GrantId {
        self.grant
    }
}
impl KeyHold {
    pub(super) fn endpoint(&self) -> &PrivateEndpointIdentity {
        &self.endpoint
    }
    pub(super) fn grant(&self) -> GrantId {
        self.grant
    }
}

impl Hold {
    pub(super) fn needs_retirement_from(&self, donor: &Self) -> bool {
        self.proof.is_none()
            && donor.activation_retirement.as_ref().is_some_and(|receipt| {
                Arc::ptr_eq(&self.origin, &receipt.origin)
                    && self
                        .activation
                        .is_some_and(|activation| activation.stamp() == receipt.stamp)
            })
    }
}

impl KeyHold {
    pub(super) fn needs_retirement_from(&self, donor: &Self) -> bool {
        self.proof.is_none()
            && donor.activation_retirement.as_ref().is_some_and(|receipt| {
                Arc::ptr_eq(&self.origin, &receipt.origin)
                    && self
                        .activation
                        .is_some_and(|activation| activation.stamp() == receipt.stamp)
            })
    }
}
