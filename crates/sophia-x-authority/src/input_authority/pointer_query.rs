/// X compatibility state observed from admitted Engine routes. The anchor is
/// the surface Engine selected, never an X grab's replacement delivery target.
#[derive(Clone, Copy, Debug)]
pub(crate) struct XPointerObservation {
    pub surface_window: XResourceId,
    pub surface: sophia_protocol::SurfaceId,
    pub root_x: i16,
    pub root_y: i16,
    pub local_x: i32,
    pub local_y: i32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct XPointerQueryState {
    pub position: Option<XPointerObservation>,
    /// When the last input this namespace observed was stamped.
    ///
    /// KEPT SO A RELEASE NOBODY TIMED HAS A TIME. Every event a request
    /// carries is stamped by its submitter, and a release caused by a source
    /// departing has no request and no submitter to ask. This is the clock
    /// every other event on the same namespace was stamped from, so a
    /// release that takes it cannot travel backwards past the press it ends.
    pub last_time_msec: u32,
    pub mask: u16,
    pub horizontal_scroll_v120: i32,
    pub vertical_scroll_v120: i32,
}

impl XInputAuthorityState {
    pub(crate) fn register_query_client(&mut self, namespace: NamespaceId, client: u64) {
        let state = self.namespaces.entry(namespace).or_default();
        if state.query_scope.0.load(std::sync::atomic::Ordering::Acquire) {
            state.query_scope = OrderedQueryScope::default();
        }
        state.query_clients.insert(client);
    }

    pub(crate) fn query_namespace_active(&self, namespace: NamespaceId) -> bool {
        self.namespaces
            .get(&namespace)
            .is_some_and(|state| !state.query_clients.is_empty())
    }

    pub(crate) fn forget_query_window(&mut self, namespace: NamespaceId, window: XResourceId) {
        if let Some(state) = self.namespaces.get_mut(&namespace)
            && let Some(pointer) = &mut state.query.position
            && pointer.surface_window == window
        {
            pointer.surface_window = XResourceId::NONE;
        }
    }

    pub(crate) fn shift_query_anchor(
        &mut self,
        namespace: NamespaceId,
        old: (i32, i32),
        new: (i32, i32),
    ) {
        if let Some(state) = self.namespaces.get_mut(&namespace)
            && let Some(pointer) = &mut state.query.position
        {
            pointer.local_x = pointer.local_x.saturating_add(old.0.saturating_sub(new.0));
            pointer.local_y = pointer.local_y.saturating_add(old.1.saturating_sub(new.1));
        }
    }
    /// Places the pointer where a client asked it to go.
    ///
    /// The only writer here that is not an observation of real input:
    /// WarpPointer moves the pointer by request, and QueryPointer has to
    /// agree with it afterwards. The button mask and the clock are left
    /// alone, because a warp presses nothing and is not an event.
    pub(crate) fn warp_query_pointer(
        &mut self,
        namespace: NamespaceId,
        position: XPointerObservation,
    ) {
        self.namespaces.entry(namespace).or_default().query.position = Some(position);
    }

    pub(crate) fn pointer_query_state(&self, namespace: NamespaceId) -> XPointerQueryState {
        self.namespaces
            .get(&namespace)
            .map_or(XPointerQueryState::default(), |state| state.query)
    }

    pub(crate) fn observe_query_modifiers(&mut self, namespace: NamespaceId, modifiers: u16) {
        let query = &mut self.namespaces.entry(namespace).or_default().query;
        query.mask = (query.mask & !0xff) | (modifiers & 0xff);
    }

    /// The actual aggregate-release producer clears only this button's query
    /// contribution. Motion, modifiers and other held buttons may have changed
    /// since its press; none are restored from that older observation. Missing
    /// namespace state is unavailable and is never created by cleanup.
    pub(crate) fn observe_query_button_release(
        &mut self,
        namespace: NamespaceId,
        button: u8,
    ) -> Result<(), ()> {
        let state = self.namespaces.get_mut(&namespace).ok_or(())?;
        if (1..=5).contains(&button) {
            state.query.mask &= !(1 << (button + 7));
        }
        Ok(())
    }

    pub(crate) fn observe_query_input(
        &mut self,
        namespace: NamespaceId,
        surface_window: XResourceId,
        event: crate::XAuthorityInputEvent,
    ) {
        use crate::{XAuthorityInputEvent, XAuthorityPointerEventKind};
        let time_msec = match event {
            XAuthorityInputEvent::Key(key) => key.time_msec,
            XAuthorityInputEvent::Pointer(pointer) => pointer.time_msec,
        };
        // Never rewound. Submitters stamp their own events and two sources
        // need not agree, and a clock that went backwards here would let a
        // release predate the press it ends.
        let observed = &mut self.namespaces.entry(namespace).or_default().query;
        observed.last_time_msec = observed.last_time_msec.max(time_msec);
        let pointer = match event {
            XAuthorityInputEvent::Key(key) => {
                self.observe_query_modifiers(namespace, u16::from(key.modifiers_after));
                return;
            }
            XAuthorityInputEvent::Pointer(pointer) => pointer,
        };
        let query = &mut self.namespaces.entry(namespace).or_default().query;
        query.position = Some(XPointerObservation {
            surface_window,
            surface: pointer.surface,
            root_x: pointer.root_x,
            root_y: pointer.root_y,
            local_x: i32::from(pointer.event_x),
            local_y: i32::from(pointer.event_y),
        });
        query.mask = pointer.state;
        match pointer.kind {
            XAuthorityPointerEventKind::Motion => {}
            XAuthorityPointerEventKind::Button { button, pressed }
            | XAuthorityPointerEventKind::Axis {
                button, pressed, ..
            } => {
                let bit = if (1..=5).contains(&button) {
                    1 << (button + 7)
                } else {
                    0
                };
                if pressed {
                    query.mask |= bit;
                } else {
                    query.mask &= !bit;
                }
            }
        }
        if let XAuthorityPointerEventKind::Axis {
            horizontal_position_v120,
            vertical_position_v120,
            ..
        } = pointer.kind
        {
            if let Some(value) = horizontal_position_v120 {
                query.horizontal_scroll_v120 = value;
            }
            if let Some(value) = vertical_position_v120 {
                query.vertical_scroll_v120 = value;
            }
        }
    }
}
