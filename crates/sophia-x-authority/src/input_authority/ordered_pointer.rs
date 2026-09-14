/// A recipient decision kept under the exclusive grab-state borrow until the
/// admitted press commits. Dropping it has no effect. In particular, a failed
/// delivery binding must not activate a passive or implicit grab.
#[cfg_attr(not(test), allow(dead_code))] // Final guarded consumer integration owns the call sites.
pub(crate) struct PreparedPointerPress<'a> {
    authority: &'a mut XInputAuthorityState,
    namespace: NamespaceId,
    selected: XActiveInputGrab,
    activation: Option<(bool, u8)>,
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
        if let Some((implicit, button)) = self.activation {
            let state = self
                .authority
                .namespaces
                .get_mut(&self.namespace)
                .expect("preparation retains exclusive ownership of this namespace");
            state.pointer = Some(self.selected);
            state.pointer_implicit = implicit;
            state.pointer_passive_detail = (!implicit).then_some(button);
            state.pointer_frozen = self.selected.pointer_mode == 0;
            state.keyboard_frozen |= self.selected.keyboard_mode == 0;
        }
        self.selected
    }
}

impl XInputAuthorityState {
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
    ) -> Option<PreparedPointerPress<'_>> {
        let state = self.namespaces.get_mut(&namespace)?;
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
        Some(PreparedPointerPress {
            authority: self,
            namespace,
            selected,
            activation,
        })
    }
}

