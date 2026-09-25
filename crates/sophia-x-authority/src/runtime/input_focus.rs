// The input focus of each namespace: preparation, validation, timestamps,
// reversion when the focus window stops being viewable, and the active
// window the focus names. Included by runtime.rs; one module with it.

impl XAuthorityRuntime {
    /// Reserves the namespace's default focus storage during connection setup,
    /// before any worker or common-held private focus producer is exposed.
    /// Existing focus survives another connection in the same namespace. This
    /// prepares storage only; it publishes no applied-focus authority.
    pub(crate) fn prepare_input_focus_namespace(&mut self, namespace: NamespaceId) {
        self.input_focus.entry(namespace).or_insert((
            crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1),
            1,
        ));
    }

    #[cfg(unix)]
    pub(crate) fn bind_private_focus_source(
        &mut self,
        source: crate::x11_socket::XPrivateFocusRuntimeSource,
    ) -> Result<(), XAuthorityRuntimeError> {
        if let Some(bound) = self.private_focus_source.as_ref() {
            if !bound.same_origin(&source) {
                return Err(XAuthorityRuntimeError::FocusAuthorityUnavailable);
            }
        } else {
            self.private_focus_source = Some(source);
        }
        Ok(())
    }

    pub fn input_focus(&self, namespace: NamespaceId) -> (crate::XResourceId, u8) {
        self.input_focus.get(&namespace).copied().unwrap_or((
            crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1),
            1,
        ))
    }

    /// Private effect entry: preparation is an admission prerequisite, never
    /// permission to allocate missing namespace state while common is held.
    pub(crate) fn set_prepared_input_focus(
        &mut self,
        namespace: NamespaceId,
        focus: crate::XResourceId,
        revert_to: u8,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.validate_input_focus(namespace, focus, revert_to)?;
        let prepared = self
            .input_focus
            .get_mut(&namespace)
            .ok_or(XAuthorityRuntimeError::FocusAuthorityUnavailable)?;
        *prepared = (focus, revert_to);
        Ok(())
    }

    /// Everything SetInputFocus must refuse, with no state changed either way.
    ///
    /// Split out from the effect because X11 orders the two: the errors are
    /// reported whatever the request's timestamp says, and only a request
    /// that would otherwise succeed is then measured against the clock.
    pub fn validate_input_focus(
        &self,
        namespace: NamespaceId,
        focus: crate::XResourceId,
        revert_to: u8,
    ) -> Result<(), XAuthorityRuntimeError> {
        // The order is the protocol's and two passing conformance purposes
        // depend on it: the out-of-range argument first, then the window that
        // does not exist, then the window that exists but cannot be focused.
        if revert_to > 2 {
            return Err(XAuthorityRuntimeError::InvalidValue);
        }
        let raw = focus.local.raw();
        if raw == u64::from(crate::X_FOCUS_NONE) || raw == u64::from(crate::X_FOCUS_POINTER_ROOT) {
            return Ok(());
        }
        if raw == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
            // The root is always viewable and is not a client resource.
            return Ok(());
        }
        self.validate_window_access(namespace, focus)?;
        // Focus is where keyboard input goes, and input cannot go to something
        // nobody can see. A window is viewable only when it and every ancestor
        // are mapped, which the window store already tracks and propagates.
        if self.window_map_state(namespace, focus)? != crate::XMapState::Viewable {
            return Err(XAuthorityRuntimeError::WindowNotViewable);
        }
        Ok(())
    }

    /// The namespace's last-focus-change time, zero before its first change.
    #[must_use]
    pub fn last_focus_change(&self, namespace: NamespaceId) -> crate::XTimestamp {
        self.last_focus_change
            .get(&namespace)
            .copied()
            .unwrap_or(crate::X_CURRENT_TIME)
    }

    /// Whether a focus request bearing `time` may take effect, and at what
    /// instant it would be recorded.
    ///
    /// X11 orders focus changes by the client's stated time rather than by
    /// arrival, so a request is discarded outright when it names a moment
    /// before the last change this namespace already made, or one the server
    /// has not reached yet. Discarded is not an error: the protocol says such
    /// a request has no effect, so the client is owed no reply, no error and
    /// no events. `CurrentTime` is the client declining to name a moment and
    /// passes both bounds, taking the server's own clock as its instant.
    #[must_use]
    pub fn focus_time_admits(
        &self,
        namespace: NamespaceId,
        time: crate::XTimestamp,
        server_time: crate::XTimestamp,
    ) -> Option<crate::XTimestamp> {
        if time == crate::X_CURRENT_TIME {
            return Some(server_time);
        }
        if crate::x_time_is_after(self.last_focus_change(namespace), time)
            || crate::x_time_is_after(time, server_time)
        {
            return None;
        }
        Some(time)
    }

    /// Records the instant a focus change took effect at. Only a request that
    /// names a time does this: reversion moves the focus without moving the
    /// clock, so a later request that was honest when it was sent still lands.
    pub fn note_focus_change(&mut self, namespace: NamespaceId, time: crate::XTimestamp) {
        self.last_focus_change.insert(namespace, time);
    }

    /// The chain from the root down to `window`, root first.
    pub(crate) fn window_ancestry_chain(
        &self,
        window: crate::XResourceId,
    ) -> Vec<crate::XResourceId> {
        let mut chain = vec![window];
        let mut candidate = window;
        // The store's parent links are bounded and this walk is cycle-guarded,
        // because a cycle here would hang a request rather than fail one.
        for _ in 0..64 {
            let Some(parent) = self.windows.get(candidate).map(|record| record.parent) else {
                break;
            };
            if parent == candidate || chain.contains(&parent) {
                break;
            }
            chain.push(parent);
            candidate = parent;
            if parent.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
                break;
            }
        }
        chain.reverse();
        chain
    }

    /// Whether a window may hold the focus: the root always may, and any other
    /// window only while it and all its ancestors are mapped.
    fn window_holds_focus_viewably(&self, window: crate::XResourceId) -> bool {
        let raw = window.local.raw();
        if raw == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
            return true;
        }
        self.windows
            .get(window)
            .is_some_and(|record| record.map_state == crate::XMapState::Viewable)
    }

    /// Moves the focus off a window that has stopped being viewable.
    ///
    /// X11 does not leave the focus on a window nobody can see. Where it goes
    /// is the revert_to the client supplied when it took the focus, which is
    /// why revert_to exists at all. This runs after anything that can change
    /// viewability, and does nothing in the ordinary case where the focus is
    /// still fine.
    ///
    /// The last-focus-change time is deliberately left alone: reverting is the
    /// server acting, not a client naming a moment, so a request that was
    /// honest when it was sent still lands afterwards.
    pub(crate) fn revert_focus_if_unviewable(&mut self, namespace: NamespaceId) {
        let (focus, revert_to) = self.input_focus(namespace);
        let old = crate::XFocusTarget::from_resource(focus);
        let crate::XFocusTarget::Window(window) = old else {
            // None and PointerRoot are not windows and cannot stop being
            // viewable, so there is nothing to revert from.
            return;
        };
        if self.window_holds_focus_viewably(window) {
            return;
        }
        let ancestry = |candidate: crate::XResourceId| self.window_ancestry_chain(candidate);
        let viewable = |candidate: crate::XResourceId| self.window_holds_focus_viewably(candidate);
        let chains = crate::XFocusChains {
            root: crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1),
            pointer: crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1),
            ancestry: &ancestry,
        };
        let (new, new_revert_to) = crate::x_focus_reversion(revert_to, window, &chains, &viewable);
        // The events are computed here, while the window tree still holds the
        // chain they are described over. A destroy removes the record before
        // this runs, so its old focus stands alone, which is the honest
        // reading of a window that no longer exists.
        let events = crate::x_focus_transition_events(old, new, &chains);
        self.input_focus
            .insert(namespace, (new.to_resource(), new_revert_to));
        self.note_active_window(namespace, new.to_resource());
        if !events.is_empty() {
            self.focus_reversions.push((namespace, events));
        }
    }

    /// Takes the reversions made since the last drain, for the layer that can
    /// reach a client's event selections to turn into records.
    pub fn take_focus_reversions(
        &mut self,
    ) -> Vec<(NamespaceId, Vec<crate::XFocusTransitionEvent>)> {
        core::mem::take(&mut self.focus_reversions)
    }

    pub fn set_input_focus(
        &mut self,
        namespace: NamespaceId,
        focus: crate::XResourceId,
        revert_to: u8,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.validate_input_focus(namespace, focus, revert_to)?;
        self.input_focus.insert(namespace, (focus, revert_to));
        self.note_active_window(namespace, focus);
        Ok(())
    }

    /// What `_NET_ACTIVE_WINDOW` should now say for this focus: the window,
    /// or 0 for `None`, `PointerRoot` and the root, which is EWMH for none.
    fn note_active_window(&mut self, namespace: NamespaceId, focus: crate::XResourceId) {
        let raw = focus.local.raw();
        let window = if raw <= 1 || raw == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
            0
        } else {
            u32::try_from(raw).unwrap_or(0)
        };
        self.active_window_changes.push((namespace, window));
    }

    /// The focus changes since the last drain, for whoever holds the
    /// property table to publish. Empty on the ordinary request.
    pub fn take_active_window_changes(&mut self) -> Vec<(NamespaceId, u32)> {
        core::mem::take(&mut self.active_window_changes)
    }
}
