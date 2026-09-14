/// Refusals before an ordered keyboard grab effect. Selection failures retain
/// their source cause; none of these answers is a common request completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyboardPreparationRefusal {
    NamespaceUnprepared,
    InvalidKey,
    InvalidModifiers,
    KeyboardFrozen,
    IdentityExhausted,
    ProvenanceUnavailable,
    FocusNotApplied,
    FocusNotViewable,
    PointerNotApplied,
    AmbiguousPassive,
    Applied(crate::x11_socket::PrivateAppliedRefusal),
}

/// Exclusive native preparation plus the actual applied topology borrows.
/// It selects no implicit keyboard grab. Dropping it changes no grab or freeze
/// state; a reserved activation serial is deliberately never reused.
///
/// This is a native component, not input admission. Its future native owner
/// must bind the selected recipient and apply the common aggregate before
/// committing; a duplicate or joined common press must not commit it.
#[allow(dead_code)] // Guarded key execution is not enabled yet.
pub(crate) struct PreparedKeyboardPress<'a> {
    authority: &'a mut XInputAuthorityState,
    topology: crate::x11_socket::PrivateKeyboardTopology<'a>,
    selected: Option<KeyboardActivation>,
    activate: bool,
}

#[allow(dead_code)]
impl PreparedKeyboardPress<'_> {
    pub(crate) fn authority(&self) -> &XInputAuthorityState {
        self.authority
    }

    pub(crate) fn recipient(&self) -> Option<XActiveInputGrab> {
        self.selected.map(KeyboardActivation::recipient)
    }

    /// Existing or reserved name, not an assertion that preparation applied
    /// the activation. Only commit returns the actual activation observation.
    pub(crate) fn reserved_stamp(&self) -> Option<KeyboardActivationStamp> {
        self.selected.map(KeyboardActivation::stamp)
    }

    /// No allocation, lookup outside the retained namespace, or fallible
    /// selection remains. The actual fields publish after a write-ahead mark.
    pub(crate) fn commit(self) -> Option<KeyboardActivation> {
        if self.activate {
            let activation = self.selected.expect("new passive activation was selected");
            let state = self
                .authority
                .namespaces
                .get_mut(&self.topology.namespace())
                .expect("preparation retains the exclusive namespace borrow");
            state.keyboard_activation = KeyboardActivationState::Changing;
            state.keyboard = Some(activation.recipient);
            state.keyboard_passive_detail = activation.trigger;
            state.keyboard_frozen = activation.recipient.keyboard_mode == 0;
            state.pointer_frozen |= activation.recipient.pointer_mode == 0;
            state.keyboard_activation = KeyboardActivationState::Applied(activation.stamp);
        }
        self.selected
    }
}

impl XInputAuthorityState {
    /// The topology can only be built from the actual bound applied source;
    /// it borrows that publication and its exact selection state until commit.
    /// This method also retains the exclusive authority borrow, so another
    /// grab producer cannot replace the selected activation during binding.
    #[allow(dead_code)] // Native key integration will own the production call.
    pub(crate) fn prepare_keyboard_press<'a>(
        &'a mut self,
        key: u8,
        modifiers: u16,
        topology: crate::x11_socket::PrivateKeyboardTopology<'a>,
    ) -> Result<PreparedKeyboardPress<'a>, KeyboardPreparationRefusal> {
        use KeyboardPreparationRefusal as R;
        if key < 8 {
            return Err(R::InvalidKey);
        }
        if modifiers & !0xff != 0 {
            return Err(R::InvalidModifiers);
        }
        let namespace = topology.namespace();
        let state = self
            .namespaces
            .get(&namespace)
            .ok_or(R::NamespaceUnprepared)?;
        let active = self
            .keyboard_activation(namespace)
            .map_err(|cause| match cause {
                KeyboardActivationRefusal::NamespaceUnprepared => R::NamespaceUnprepared,
                KeyboardActivationRefusal::ProvenanceUnavailable => R::ProvenanceUnavailable,
            })?;
        if state.keyboard_frozen {
            // Frozen logical input needs the ordered StateOnly/thaw path;
            // a new key cannot silently clear another grab's contribution.
            return Err(R::KeyboardFrozen);
        }
        // Active ownership precedes passive policy, including a newer matching
        // registration. The existing ordinary activate_key entry is unchanged.
        let (selected, activate) = if let Some(active) = active {
            (Some(active), false)
        } else if let Some(passive) =
            topology.select(&state.keys, key, modifiers, state.query.position)?
        {
            let stamp = KeyboardActivationStamp::reserve(
                &mut self.keyboard_activation_high_water,
                namespace,
            )
            .ok_or(R::IdentityExhausted)?;
            (
                Some(KeyboardActivation {
                    stamp,
                    recipient: active_from_passive(passive),
                    trigger: Some(key),
                }),
                true,
            )
        } else {
            (None, false)
        };
        Ok(PreparedKeyboardPress {
            authority: self,
            topology,
            selected,
            activate,
        })
    }
}
