/// An activation name scoped to one origin's native authority. The owner of
/// that authority supplies the origin binding; the namespace and checked
/// serial distinguish replacement grabs, including identical replacements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PointerActivationStamp {
    namespace: NamespaceId,
    serial: u64,
}

impl PointerActivationStamp {
    fn reserve(high_water: &mut u64, namespace: NamespaceId) -> Option<Self> {
        let serial = high_water.checked_add(1)?;
        *high_water = serial;
        Some(Self { namespace, serial })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum PointerActivationState {
    #[default]
    Absent,
    // Written before any field of an activation changes. An interrupted change
    // is unavailable, not an absent grab or a completed activation.
    Changing,
    Applied(PointerActivationStamp),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointerPreparationRefusal {
    NamespaceUnprepared,
    IdentityExhausted,
    ProvenanceUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PointerActivationCommit {
    recipient: XActiveInputGrab,
    stamp: PointerActivationStamp,
    automatic: bool,
}

#[cfg_attr(not(test), allow(dead_code))] // Consumed by the native proof producer.
impl PointerActivationCommit {
    pub(crate) fn recipient(self) -> XActiveInputGrab { self.recipient }
    pub(crate) fn stamp(self) -> PointerActivationStamp { self.stamp }
    pub(crate) fn automatic(self) -> bool { self.automatic }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointerActivationRetirement {
    Retired,
    StillRequiredByOtherButtons,
    AlreadyAbsent,
    Replaced,
    Explicit,
    Unavailable,
    LeaseUnproved,
    SynchronousUnproved,
}

/// A recipient decision kept under the exclusive grab-state borrow until the
/// admitted press commits. Dropping it has no native effect (its reserved
/// serial is never reused). In particular, a failed
/// delivery binding must not activate a passive or implicit grab.
#[cfg_attr(not(test), allow(dead_code))] // Final guarded consumer integration owns the call sites.
pub(crate) struct PreparedPointerPress<'a> {
    authority: &'a mut XInputAuthorityState,
    namespace: NamespaceId,
    selected: XActiveInputGrab,
    activation: Option<(bool, u8)>,
    stamp: PointerActivationStamp,
    automatic: bool,
}

#[cfg_attr(not(test), allow(dead_code))]
impl PreparedPointerPress<'_> {
    pub(crate) fn namespace(&self) -> NamespaceId {
        self.namespace
    }

    pub(crate) fn authority(&self) -> &XInputAuthorityState {
        self.authority
    }

    pub(crate) fn is_new_implicit(&self) -> bool {
        matches!(self.activation, Some((true, _)))
    }

    /// Refine a proposed implicit grab to the window selected by the guarded
    /// event resolver. This changes no native state and must precede binding.
    pub(crate) fn with_implicit_window(mut self, window: XResourceId) -> Result<Self, Self> {
        if !self.is_new_implicit() {
            return Err(self);
        }
        self.selected.window = window;
        Ok(self)
    }

    pub(crate) fn recipient(&self) -> XActiveInputGrab {
        self.selected
    }

    /// Called only for a ledger press that began the aggregate. A join drops
    /// this preparation and keeps the incarnation's already recorded target.
    pub(crate) fn commit(self) -> XActiveInputGrab {
        self.commit_stamped().recipient
    }

    /// Preparation already reserved the serial and retains the exclusive
    /// namespace borrow; this commit has no remaining allocation or refusal.
    pub(crate) fn commit_stamped(self) -> PointerActivationCommit {
        if let Some((implicit, button)) = self.activation {
            let state = self
                .authority
                .namespaces
                .get_mut(&self.namespace)
                .expect("preparation retains exclusive ownership of this namespace");
            state.pointer_activation = PointerActivationState::Changing;
            state.pointer = Some(self.selected);
            state.pointer_implicit = implicit;
            state.pointer_passive_detail = (!implicit).then_some(button);
            state.pointer_frozen = self.selected.pointer_mode == 0;
            state.keyboard_frozen |= self.selected.keyboard_mode == 0;
            state.pointer_activation = PointerActivationState::Applied(self.stamp);
        }
        PointerActivationCommit {
            recipient: self.selected,
            stamp: self.stamp,
            automatic: self.automatic,
        }
    }
}

impl XInputAuthorityState {
    #[cfg_attr(not(test), allow(dead_code))] // Native query updates may not create a namespace.
    pub(crate) fn has_ordered_namespace(&self, namespace: NamespaceId) -> bool {
        self.namespaces.contains_key(&namespace)
    }

    /// Initialization belongs before private producer exposure. Ordered
    /// transactions refuse an absent namespace instead of allocating one.
    pub(crate) fn prepare_ordered_namespace(&mut self, namespace: NamespaceId) {
        self.namespaces.entry(namespace).or_default();
    }

    /// Selection and commit share one exclusive borrow. The caller retains
    /// its ranked authority guard through binding and common application.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn prepare_pointer_press(
        &mut self,
        namespace: NamespaceId,
        button: u8,
        modifiers: u16,
        implicit: XActiveInputGrab,
    ) -> Result<PreparedPointerPress<'_>, PointerPreparationRefusal> {
        let state = self.namespaces.get(&namespace)
            .ok_or(PointerPreparationRefusal::NamespaceUnprepared)?;
        if state.pointer_activation == PointerActivationState::Changing {
            return Err(PointerPreparationRefusal::ProvenanceUnavailable);
        }
        let (selected, activation) = if let Some(active) = state.pointer {
            (active, None)
        } else {
            let passive = state.buttons.iter().copied().find(|grab| {
                (grab.detail == 0 || grab.detail == button)
                    && (grab.modifiers == X_ANY_MODIFIER || grab.modifiers == modifiers)
            });
            match passive {
                Some(passive) => (active_from_passive(passive), Some((false, button))),
                None => (implicit, Some((true, button))),
            }
        };
        let automatic = activation.is_some()
            || state.pointer_implicit || state.pointer_passive_detail.is_some();
        let stamp = if activation.is_some() {
            PointerActivationStamp::reserve(&mut self.pointer_activation_high_water, namespace)
                .ok_or(PointerPreparationRefusal::IdentityExhausted)?
        } else if let PointerActivationState::Applied(stamp) = state.pointer_activation {
            stamp
        } else {
            return Err(PointerPreparationRefusal::ProvenanceUnavailable);
        };
        Ok(PreparedPointerPress {
            authority: self,
            namespace,
            selected,
            activation,
            stamp,
            automatic,
        })
    }

    /// Retire only the exact automatic grab this hold reached. The native
    /// owner supplies its own held mapper; a zero core wire mask cannot prove
    /// that side buttons are up. Absence/replacement are observations, not
    /// receipts that this method completed an earlier retirement.
    #[cfg_attr(not(test), allow(dead_code))] // Guarded native release integration.
    pub(crate) fn retire_pointer_activation(
        &mut self,
        namespace: NamespaceId,
        stamp: PointerActivationStamp,
        released_button: u8,
        pointer: &crate::XCorePointerMapper,
    ) -> PointerActivationRetirement {
        use PointerActivationRetirement as R;
        if stamp.namespace != namespace { return R::Replaced; }
        let Some(state) = self.namespaces.get_mut(&namespace) else { return R::Unavailable; };
        match state.pointer_activation {
            PointerActivationState::Changing => return R::Unavailable,
            PointerActivationState::Absent => return R::AlreadyAbsent,
            PointerActivationState::Applied(current) if current != stamp => return R::Replaced,
            PointerActivationState::Applied(_) => {}
        }
        let Some(grab) = state.pointer else { return R::Unavailable; };
        if !state.pointer_implicit && state.pointer_passive_detail.is_none() { return R::Explicit; }
        if grab.route_lease.is_some() { return R::LeaseUnproved; }
        if grab.pointer_mode == 0 || grab.keyboard_mode == 0
            || state.pointer_frozen || state.keyboard_frozen { return R::SynchronousUnproved; }
        if pointer.button_is_pressed(released_button)
            || (state.pointer_implicit && !pointer.all_buttons_released())
            || (!state.pointer_implicit && state.pointer_passive_detail != Some(released_button)) {
            return R::StillRequiredByOtherButtons;
        }
        state.pointer_activation = PointerActivationState::Changing;
        state.pointer = None;
        state.pointer_implicit = false;
        state.pointer_passive_detail = None;
        state.pointer_frozen = false;
        state.pointer_activation = PointerActivationState::Absent;
        R::Retired
    }
}
