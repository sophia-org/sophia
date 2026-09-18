/// Source-only keyboard topology. Keeping these borrows inside the exclusive
/// grab preview prevents a queued focus intent or a caller-built ancestry from
/// supplying passive-grab eligibility. This is not a key-execution permit.
#[cfg(unix)]
#[allow(dead_code)] // Native key source integration is still required.
mod private_keyboard_topology {
    use super::*;
    use crate::{KeyboardPreparationRefusal as R, XPassiveInputGrab};

    pub(crate) struct Topology<'a> {
        state: &'a PrivateAppliedRoutingState,
        selections: &'a XCoreEventSelectionState,
    }

    impl<'a> Topology<'a> {
        pub(super) fn from_applied(
            state: &'a PrivateAppliedRoutingState,
            client: XServerFrontendClientId,
            selections: &'a XCoreEventSelectionState,
        ) -> Result<Self, R> {
            if !state.published {
                return Err(R::Applied(PrivateAppliedRefusal::Unpublished));
            }
            if selections.private_origin
                != Some(PrivateAppliedSelectionOrigin {
                    authority: state.authority,
                    namespace: state.namespace,
                    client,
                })
            {
                return Err(R::Applied(PrivateAppliedRefusal::ForeignOrigin));
            }
            if selections.applied_revision.is_none() {
                return Err(R::Applied(PrivateAppliedRefusal::Interrupted));
            }
            if state.focus.is_some_and(|focus| focus.client != client) {
                return Err(R::Applied(PrivateAppliedRefusal::ForeignOrigin));
            }
            Ok(Self { state, selections })
        }

        pub(crate) fn namespace(&self) -> NamespaceId {
            self.state.namespace
        }

        pub(crate) fn select(
            &self,
            grabs: &[XPassiveInputGrab],
            key: u8,
            modifiers: u16,
            pointer: Option<crate::XPointerObservation>,
        ) -> Result<Option<XPassiveInputGrab>, R> {
            let focus = self.state.focus.ok_or(R::FocusNotApplied)?;
            let budget = PrivateTraversalBudget::new();
            budget.charge(grabs.len()).map_err(R::Applied)?;
            let focus_path = self
                .selections
                .ordered_ancestry_budget(focus.window, &budget)
                .map_err(R::Applied)?;
            let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
            if focus_path
                .as_slice()
                .iter()
                .any(|window| *window != root && !self.selections.mapped.contains(window))
            {
                return Err(R::FocusNotViewable);
            }
            // Root-most matching focus ancestor wins, independently of the
            // registration order. It outranks every pointer descendant, so no
            // pointer observation is needed when this path decides the grab.
            if let Some(grab) = self.on_path(grabs, key, modifiers, &focus_path, &budget)? {
                return Ok(Some(grab));
            }
            budget.charge(grabs.len()).map_err(R::Applied)?;
            if !grabs
                .iter()
                .any(|grab| Self::matches(*grab, key, modifiers))
            {
                return Ok(None);
            }
            // Absence of pointer evidence cannot prove that a descendant grab
            // is ineligible. The native authority supplies this observation;
            // callers cannot replace it with their own coordinates.
            let pointer = pointer.ok_or(R::PointerNotApplied)?;
            if pointer.surface_window != root {
                let geometry = self
                    .selections
                    .geometries
                    .get(&pointer.surface_window)
                    .ok_or(R::Applied(PrivateAppliedRefusal::HierarchyMissing))?;
                if pointer.local_x < 0
                    || pointer.local_y < 0
                    || pointer.local_x >= geometry.width
                    || pointer.local_y >= geometry.height
                {
                    return Err(R::PointerNotApplied);
                }
            }
            let local_x = i16::try_from(pointer.local_x)
                .map_err(|_| R::Applied(PrivateAppliedRefusal::CoordinateOverflow))?;
            let local_y = i16::try_from(pointer.local_y)
                .map_err(|_| R::Applied(PrivateAppliedRefusal::CoordinateOverflow))?;
            let target = self
                .selections
                .ordered_pointer_target_budget(pointer.surface_window, local_x, local_y, &budget)
                .map_err(R::Applied)?;
            let pointer_path = self
                .selections
                .ordered_ancestry_budget(target, &budget)
                .map_err(R::Applied)?;
            if pointer_path
                .as_slice()
                .iter()
                .any(|window| *window != root && !self.selections.mapped.contains(window))
            {
                return Err(R::PointerNotApplied);
            }
            if !pointer_path.as_slice().contains(&focus.window) {
                return Ok(None);
            }
            self.on_path(grabs, key, modifiers, &pointer_path, &budget)
        }

        fn matches(grab: XPassiveInputGrab, key: u8, modifiers: u16) -> bool {
            (grab.detail == 0 || grab.detail == key)
                && (grab.modifiers == crate::X_ANY_MODIFIER || grab.modifiers == modifiers)
        }

        fn on_path(
            &self,
            grabs: &[XPassiveInputGrab],
            key: u8,
            modifiers: u16,
            path: &PrivateOrderedAncestry,
            budget: &PrivateTraversalBudget,
        ) -> Result<Option<XPassiveInputGrab>, R> {
            let mut selected: Option<(usize, XPassiveInputGrab)> = None;
            let mut ambiguous = false;
            budget.charge(grabs.len()).map_err(R::Applied)?;
            for grab in grabs
                .iter()
                .copied()
                .filter(|grab| Self::matches(*grab, key, modifiers))
            {
                budget.charge(path.as_slice().len()).map_err(R::Applied)?;
                let Some(depth) = path.depth(grab.window) else {
                    continue;
                };
                match selected {
                    Some((old_depth, old)) if depth == old_depth && old != grab => ambiguous = true,
                    Some((old_depth, _)) if depth <= old_depth => {}
                    _ => {
                        selected = Some((depth, grab));
                        ambiguous = false;
                    }
                }
            }
            if ambiguous {
                return Err(R::AmbiguousPassive);
            }
            Ok(selected.map(|(_, grab)| grab))
        }
    }
}

#[cfg(unix)]
pub(crate) use private_keyboard_topology::Topology as PrivateKeyboardTopology;
