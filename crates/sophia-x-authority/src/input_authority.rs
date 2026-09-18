use std::collections::BTreeMap;

use sophia_protocol::NamespaceId;

use crate::XResourceId;

include!("input_authority/pointer_query.rs");
include!("input_authority/ordered_pointer.rs");
include!("input_authority/ordered_keyboard.rs");
include!("input_authority/ordered_freeze.rs");
include!("input_authority/ordered_keyboard_retirement.rs");
include!("input_authority/owner_cleanup.rs");
#[cfg(unix)]
include!("input_authority/ordered_keyboard_press.rs");

pub const X_ANY_MODIFIER: u16 = 0x8000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XInputGrabError {
    AlreadyGrabbed,
    AccessConflict,
    InvalidMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XActiveInputGrab {
    pub owner: u64,
    pub window: XResourceId,
    pub owner_events: bool,
    pub pointer_mode: u8,
    pub keyboard_mode: u8,
    pub event_mask: u16,
    pub xi_event_mask: [u32; 8],
    pub xi_event_mask_words: u8,
    pub route_lease: Option<sophia_protocol::ApplicationRouteLeaseIdentity>,
}

impl XActiveInputGrab {
    pub fn selects_xi_event(self, event_type: u16) -> bool {
        let word = usize::from(event_type / 32);
        word < usize::from(self.xi_event_mask_words)
            && self.xi_event_mask[word] & (1_u32 << (event_type % 32)) != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPassiveInputGrab {
    pub owner: u64,
    pub window: XResourceId,
    pub detail: u8,
    pub modifiers: u16,
    pub owner_events: bool,
    pub pointer_mode: u8,
    pub keyboard_mode: u8,
    pub event_mask: u16,
}

#[derive(Clone, Debug, Default)]
struct XNamespaceInputAuthority {
    query: XPointerQueryState,
    query_scope: OrderedQueryScope,
    query_clients: std::collections::BTreeSet<u64>,
    pointer: Option<XActiveInputGrab>,
    pointer_activation: PointerActivationState,
    keyboard: Option<XActiveInputGrab>,
    keyboard_activation: KeyboardActivationState,
    buttons: Vec<XPassiveInputGrab>,
    keys: Vec<XPassiveInputGrab>,
    server_owner: Option<u64>,
    /// Connections parked because another client holds the server grab.
    server_grab_waiters: crate::connection_wait::NotifierRegistry,
    freeze: OrderedFreezeState,
    pointer_implicit: bool,
    pointer_passive_detail: Option<u8>,
    keyboard_passive_detail: Option<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct XInputAuthorityState {
    namespaces: BTreeMap<NamespaceId, XNamespaceInputAuthority>,
    xi_selections: BTreeMap<(NamespaceId, u64, XResourceId, u16), Vec<u32>>,
    // Kept outside namespace storage: removing and recreating a namespace must
    // not make an old automatic-grab identity current again.
    pointer_activation_high_water: u64,
    keyboard_activation_high_water: u64,
}

impl XInputAuthorityState {
    pub fn grab_pointer(
        &mut self,
        namespace: NamespaceId,
        grab: XActiveInputGrab,
    ) -> Result<(), XInputGrabError> {
        validate_modes(grab.pointer_mode, grab.keyboard_mode)?;
        let state = self.namespaces.entry(namespace).or_default();
        if state
            .pointer
            .is_some_and(|active| active.owner != grab.owner)
        {
            return Err(XInputGrabError::AlreadyGrabbed);
        }
        let activation =
            PointerActivationStamp::reserve(&mut self.pointer_activation_high_water, namespace);
        state.pointer_activation = PointerActivationState::Changing;
        state.pointer = Some(grab);
        state.pointer_implicit = false;
        state.pointer_passive_detail = None;
        state.freeze.activate_pointer(activation, grab);
        state.pointer_activation = activation.map_or(
            PointerActivationState::Changing,
            PointerActivationState::Applied,
        );
        Ok(())
    }

    pub fn ungrab_pointer(&mut self, namespace: NamespaceId, owner: u64) {
        if let Some(state) = self.namespaces.get_mut(&namespace)
            && state.pointer.is_some_and(|grab| grab.owner == owner)
        {
            state.pointer_activation = PointerActivationState::Changing;
            state.pointer = None;
            state.pointer_implicit = false;
            state.pointer_passive_detail = None;
            state.freeze.pointer = None;
            state.pointer_activation = PointerActivationState::Absent;
        }
    }

    pub fn grab_keyboard(
        &mut self,
        namespace: NamespaceId,
        grab: XActiveInputGrab,
    ) -> Result<(), XInputGrabError> {
        validate_modes(grab.pointer_mode, grab.keyboard_mode)?;
        let state = self.namespaces.entry(namespace).or_default();
        if state
            .keyboard
            .is_some_and(|active| active.owner != grab.owner)
        {
            return Err(XInputGrabError::AlreadyGrabbed);
        }
        let activation =
            KeyboardActivationStamp::reserve(&mut self.keyboard_activation_high_water, namespace);
        state.keyboard_activation = KeyboardActivationState::Changing;
        state.keyboard = Some(grab);
        state.keyboard_passive_detail = None;
        state.freeze.activate_keyboard(activation, grab);
        state.keyboard_activation = activation.map_or(
            KeyboardActivationState::Changing,
            KeyboardActivationState::Applied,
        );
        Ok(())
    }

    pub fn ungrab_keyboard(&mut self, namespace: NamespaceId, owner: u64) {
        if let Some(state) = self.namespaces.get_mut(&namespace)
            && state.keyboard.is_some_and(|grab| grab.owner == owner)
        {
            state.keyboard_activation = KeyboardActivationState::Changing;
            state.keyboard = None;
            state.keyboard_passive_detail = None;
            state.freeze.keyboard = None;
            state.keyboard_activation = KeyboardActivationState::Absent;
        }
    }

    pub fn grab_button(
        &mut self,
        namespace: NamespaceId,
        grab: XPassiveInputGrab,
    ) -> Result<(), XInputGrabError> {
        validate_modes(grab.pointer_mode, grab.keyboard_mode)?;
        insert_passive(
            &mut self.namespaces.entry(namespace).or_default().buttons,
            grab,
        )
    }

    pub fn grab_key(
        &mut self,
        namespace: NamespaceId,
        grab: XPassiveInputGrab,
    ) -> Result<(), XInputGrabError> {
        validate_modes(grab.pointer_mode, grab.keyboard_mode)?;
        insert_passive(
            &mut self.namespaces.entry(namespace).or_default().keys,
            grab,
        )
    }

    pub fn ungrab_button(
        &mut self,
        namespace: NamespaceId,
        owner: u64,
        window: XResourceId,
        detail: u8,
        modifiers: u16,
    ) {
        if let Some(state) = self.namespaces.get_mut(&namespace) {
            remove_passive(&mut state.buttons, owner, window, detail, modifiers);
        }
    }

    pub fn ungrab_key(
        &mut self,
        namespace: NamespaceId,
        owner: u64,
        window: XResourceId,
        detail: u8,
        modifiers: u16,
    ) {
        if let Some(state) = self.namespaces.get_mut(&namespace) {
            remove_passive(&mut state.keys, owner, window, detail, modifiers);
        }
    }

    pub fn grab_server(
        &mut self,
        namespace: NamespaceId,
        owner: u64,
    ) -> Result<(), XInputGrabError> {
        let state = self.namespaces.entry(namespace).or_default();
        if state.server_owner.is_some_and(|active| active != owner) {
            return Err(XInputGrabError::AlreadyGrabbed);
        }
        state.server_owner = Some(owner);
        Ok(())
    }

    pub fn ungrab_server(&mut self, namespace: NamespaceId, owner: u64) {
        if let Some(state) = self.namespaces.get_mut(&namespace)
            && state.server_owner == Some(owner)
        {
            state.server_owner = None;
            state.server_grab_waiters.notify_all();
        }
    }

    /// Park a connection until this namespace's server grab is released.
    ///
    /// Registration happens under the same guard that reads the owner, so a
    /// release between the two cannot be missed.
    pub fn await_server_grab(
        &mut self,
        namespace: NamespaceId,
        notifier: &crate::connection_wait::ConnectionNotifier,
    ) {
        self.namespaces
            .entry(namespace)
            .or_default()
            .server_grab_waiters
            .register(notifier);
    }

    pub fn allow_events(
        &mut self,
        namespace: NamespaceId,
        owner: u64,
        mode: u8,
    ) -> Result<(), XInputGrabError> {
        if mode > 7 {
            return Err(XInputGrabError::InvalidMode);
        }
        let Some(state) = self.namespaces.get_mut(&namespace) else {
            return Ok(());
        };
        state.freeze.allow_events(owner, mode);
        Ok(())
    }

    pub fn pointer_grab(&self, namespace: NamespaceId) -> Option<XActiveInputGrab> {
        self.namespaces
            .get(&namespace)
            .and_then(|state| state.pointer)
    }

    pub fn set_pointer_route_lease(
        &mut self,
        namespace: NamespaceId,
        owner: u64,
        identity: sophia_protocol::ApplicationRouteLeaseIdentity,
    ) -> Result<(), XInputGrabError> {
        let Some(grab) = self
            .namespaces
            .get_mut(&namespace)
            .and_then(|state| state.pointer.as_mut())
            .filter(|grab| grab.owner == owner)
        else {
            return Err(XInputGrabError::AccessConflict);
        };
        grab.route_lease = Some(identity);
        Ok(())
    }

    pub fn keyboard_grab(&self, namespace: NamespaceId) -> Option<XActiveInputGrab> {
        self.namespaces
            .get(&namespace)
            .and_then(|state| state.keyboard)
    }

    pub fn pointer_frozen(&self, namespace: NamespaceId) -> bool {
        self.namespaces
            .get(&namespace)
            .is_some_and(|state| state.freeze.frozen(FREEZE_POINTER))
    }

    pub fn keyboard_frozen(&self, namespace: NamespaceId) -> bool {
        self.namespaces
            .get(&namespace)
            .is_some_and(|state| state.freeze.frozen(FREEZE_KEYBOARD))
    }

    /// Clears protocol-local active ownership at an Engine security epoch.
    /// Passive registrations remain namespace-local policy, but no active grab
    /// or frozen delivery is allowed to cross the transition.
    pub fn advance_security_epoch(&mut self) {
        for state in self.namespaces.values_mut() {
            state.pointer_activation = PointerActivationState::Changing;
            state.keyboard_activation = KeyboardActivationState::Changing;
            state.query = XPointerQueryState::default();
            state.pointer = None;
            state.keyboard = None;
            state.freeze = OrderedFreezeState::default();
            state.server_owner = None;
            state.server_grab_waiters.notify_all();
            state.pointer_activation = PointerActivationState::Absent;
            state.keyboard_activation = KeyboardActivationState::Absent;
        }
    }

    pub fn server_owner(&self, namespace: NamespaceId) -> Option<u64> {
        self.namespaces
            .get(&namespace)
            .and_then(|state| state.server_owner)
    }

    pub fn select_xi_events(
        &mut self,
        namespace: NamespaceId,
        owner: u64,
        window: XResourceId,
        masks: &[(u16, Vec<u32>)],
    ) {
        for (device, mask) in masks {
            let key = (namespace, owner, window, *device);
            if mask.iter().all(|word| *word == 0) {
                self.xi_selections.remove(&key);
            } else {
                self.xi_selections.insert(key, mask.clone());
            }
        }
    }

    pub fn xi_event_selected(
        &self,
        namespace: NamespaceId,
        owner: u64,
        window: XResourceId,
        device: u16,
        event_type: u16,
    ) -> bool {
        [device, 0, 1].into_iter().any(|selected_device| {
            if selected_device == 1 && !matches!(device, 2 | 3) {
                return false;
            }
            self.xi_selections
                .get(&(namespace, owner, window, selected_device))
                .is_some_and(|mask| {
                    let bit = usize::from(event_type);
                    mask.get(bit / 32)
                        .is_some_and(|word| word & (1 << (bit % 32)) != 0)
                })
        })
    }

    pub fn activate_key(
        &mut self,
        namespace: NamespaceId,
        key: u8,
        modifiers: u16,
    ) -> Option<XActiveInputGrab> {
        let state = self.namespaces.get_mut(&namespace)?;
        let passive = state.keys.iter().copied().find(|grab| {
            (grab.detail == 0 || grab.detail == key)
                && (grab.modifiers == X_ANY_MODIFIER || grab.modifiers == modifiers)
        })?;
        let active = active_from_passive(passive);
        let activation =
            KeyboardActivationStamp::reserve(&mut self.keyboard_activation_high_water, namespace);
        state.keyboard_activation = KeyboardActivationState::Changing;
        state.keyboard = Some(active);
        state.keyboard_passive_detail = Some(key);
        state.freeze.activate_keyboard(activation, active);
        state.keyboard_activation = activation.map_or(
            KeyboardActivationState::Changing,
            KeyboardActivationState::Applied,
        );
        Some(active)
    }

    pub fn release_key(&mut self, namespace: NamespaceId, key: u8) {
        if let Some(state) = self.namespaces.get_mut(&namespace)
            && state.keyboard_passive_detail == Some(key)
        {
            state.keyboard_activation = KeyboardActivationState::Changing;
            state.keyboard = None;
            state.keyboard_passive_detail = None;
            state.freeze.keyboard = None;
            state.keyboard_activation = KeyboardActivationState::Absent;
        }
    }

    pub fn activate_button(
        &mut self,
        namespace: NamespaceId,
        button: u8,
        modifiers: u16,
        implicit: XActiveInputGrab,
    ) -> XActiveInputGrab {
        let state = self.namespaces.entry(namespace).or_default();
        // A further press belongs to the active grab; it cannot activate a
        // passive grab or change an explicit grab into an automatic one.
        if let Some(active) = state.pointer {
            return active;
        }
        let (active, is_implicit) = state
            .buttons
            .iter()
            .copied()
            .find(|grab| {
                (grab.detail == 0 || grab.detail == button)
                    && (grab.modifiers == X_ANY_MODIFIER || grab.modifiers == modifiers)
            })
            .map(|grab| (active_from_passive(grab), false))
            .unwrap_or((implicit, true));
        let activation =
            PointerActivationStamp::reserve(&mut self.pointer_activation_high_water, namespace);
        state.pointer_activation = PointerActivationState::Changing;
        state.pointer = Some(active);
        state.pointer_implicit = is_implicit;
        state.pointer_passive_detail = (!is_implicit).then_some(button);
        state.freeze.activate_pointer(activation, active);
        state.pointer_activation = activation.map_or(
            PointerActivationState::Changing,
            PointerActivationState::Applied,
        );
        active
    }

    pub fn release_button(
        &mut self,
        namespace: NamespaceId,
        button: u8,
        all_core_buttons_released: bool,
    ) {
        if let Some(state) = self.namespaces.get_mut(&namespace)
            && ((state.pointer_implicit && all_core_buttons_released)
                || state.pointer_passive_detail == Some(button))
        {
            state.pointer_activation = PointerActivationState::Changing;
            state.pointer = None;
            state.pointer_implicit = false;
            state.pointer_passive_detail = None;
            state.freeze.pointer = None;
            state.pointer_activation = PointerActivationState::Absent;
        }
    }

    pub fn cleanup_owner(&mut self, owner: u64) {
        self.xi_selections
            .retain(|(_, selection_owner, _, _), _| *selection_owner != owner);
        self.namespaces.retain(|_, state| {
            state.freeze.remove_owner(owner);
            let query_removed = state.query_clients.remove(&owner);
            if state.query_clients.is_empty() {
                state.query = XPointerQueryState::default();
                if query_removed {
                    state.query_scope.0.store(true, std::sync::atomic::Ordering::Release);
                }
            }
            if state.pointer.is_some_and(|grab| grab.owner == owner) {
                state.pointer_activation = PointerActivationState::Changing;
                state.pointer = None;
                state.pointer_implicit = false;
                state.pointer_passive_detail = None;
                state.pointer_activation = PointerActivationState::Absent;
            }
            if state.keyboard.is_some_and(|grab| grab.owner == owner) {
                state.keyboard_activation = KeyboardActivationState::Changing;
                state.keyboard = None;
                state.keyboard_passive_detail = None;
                state.keyboard_activation = KeyboardActivationState::Absent;
            }
            state.buttons.retain(|grab| grab.owner != owner);
            state.keys.retain(|grab| grab.owner != owner);
            if state.server_owner == Some(owner) {
                state.server_owner = None;
                state.server_grab_waiters.notify_all();
            }
            state.pointer.is_some()
                || !state.query_clients.is_empty()
                || state.keyboard.is_some()
                || !state.buttons.is_empty()
                || !state.keys.is_empty()
                || state.server_owner.is_some()
        });
    }
}

fn active_from_passive(grab: XPassiveInputGrab) -> XActiveInputGrab {
    XActiveInputGrab {
        owner: grab.owner,
        window: grab.window,
        owner_events: grab.owner_events,
        pointer_mode: grab.pointer_mode,
        keyboard_mode: grab.keyboard_mode,
        event_mask: grab.event_mask,
        xi_event_mask: [0; 8],
        xi_event_mask_words: 0,
        route_lease: None,
    }
}

fn validate_modes(pointer: u8, keyboard: u8) -> Result<(), XInputGrabError> {
    if pointer > 1 || keyboard > 1 {
        Err(XInputGrabError::InvalidMode)
    } else {
        Ok(())
    }
}

fn patterns_overlap(a: XPassiveInputGrab, b: XPassiveInputGrab) -> bool {
    (a.detail == 0 || b.detail == 0 || a.detail == b.detail)
        && (a.modifiers == X_ANY_MODIFIER
            || b.modifiers == X_ANY_MODIFIER
            || a.modifiers == b.modifiers)
        && a.window == b.window
}

fn insert_passive(
    grabs: &mut Vec<XPassiveInputGrab>,
    grab: XPassiveInputGrab,
) -> Result<(), XInputGrabError> {
    if grabs
        .iter()
        .copied()
        .any(|existing| existing.owner != grab.owner && patterns_overlap(existing, grab))
    {
        return Err(XInputGrabError::AccessConflict);
    }
    grabs.retain(|existing| {
        !(existing.owner == grab.owner
            && existing.window == grab.window
            && existing.detail == grab.detail
            && existing.modifiers == grab.modifiers)
    });
    grabs.push(grab);
    Ok(())
}

fn remove_passive(
    grabs: &mut Vec<XPassiveInputGrab>,
    owner: u64,
    window: XResourceId,
    detail: u8,
    modifiers: u16,
) {
    grabs.retain(|grab| {
        grab.owner != owner
            || grab.window != window
            || !(detail == 0 || grab.detail == detail)
            || !(modifiers == X_ANY_MODIFIER || grab.modifiers == modifiers)
    });
}

#[path = "input_authority/tests.rs"]
mod ordered_pointer_tests;

#[path = "input_authority/tests/ordered_keyboard.rs"]
mod ordered_keyboard_tests;

#[path = "input_authority/tests/keyboard_retirement.rs"]
mod keyboard_retirement_tests;
