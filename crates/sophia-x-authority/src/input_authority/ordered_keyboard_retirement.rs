/// Result of attempting to retire one exact native keyboard activation.
/// Only Retired means this operation removed it. Absence, replacement and
/// unavailable history cannot serve as a receipt for an earlier obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyboardActivationRetirement {
    Retired,
    StillRequiredByTrigger,
    AlreadyAbsent,
    Replaced,
    Explicit,
    Unavailable,
    KeyboardUnavailable,
    LeaseUnproved,
    SynchronousUnproved,
}

impl XInputAuthorityState {
    /// The native owner must call this after the final common release and the
    /// actual XKB update, while holding this authority and its exact seat runner.
    /// The owner must validate origin/seat and retain the result with the full
    /// incarnation. This component proves neither that ownership nor native
    /// query/lease cleanup or recipient settlement.
    #[allow(dead_code)] // The native key owner will consume this operation.
    pub(crate) fn retire_keyboard_activation(
        &mut self,
        namespace: NamespaceId,
        stamp: KeyboardActivationStamp,
        released_key: u8,
        keyboard: &crate::XkbKeyboardState,
    ) -> KeyboardActivationRetirement {
        use crate::keyboard::XkbPhysicalKeyState;
        use KeyboardActivationRetirement as R;
        if stamp.namespace != namespace {
            return R::Replaced;
        }
        let Some(state) = self.namespaces.get_mut(&namespace) else {
            return R::Unavailable;
        };
        match state.keyboard_activation {
            KeyboardActivationState::Changing => return R::Unavailable,
            KeyboardActivationState::Absent if state.keyboard.is_none() => return R::AlreadyAbsent,
            KeyboardActivationState::Absent => return R::Unavailable,
            KeyboardActivationState::Applied(current) if current != stamp => return R::Replaced,
            KeyboardActivationState::Applied(_) => {}
        }
        let Some(grab) = state.keyboard else {
            return R::Unavailable;
        };
        let Some(trigger) = state.keyboard_passive_detail else {
            return R::Explicit;
        };
        if grab.route_lease.is_some() {
            return R::LeaseUnproved;
        }
        if grab.pointer_mode == 0
            || grab.keyboard_mode == 0
            || state.pointer_frozen
            || state.keyboard_frozen
        {
            return R::SynchronousUnproved;
        }
        match keyboard.physical_key_state(released_key) {
            XkbPhysicalKeyState::Unavailable | XkbPhysicalKeyState::InvalidKey => {
                return R::KeyboardUnavailable;
            }
            XkbPhysicalKeyState::Held => return R::StillRequiredByTrigger,
            XkbPhysicalKeyState::Released => {}
        }
        if trigger != released_key {
            return R::StillRequiredByTrigger;
        }
        state.keyboard_activation = KeyboardActivationState::Changing;
        state.keyboard = None;
        state.keyboard_passive_detail = None;
        // Both freeze contributions were proved absent above. Do not clear a
        // pointer grab, its provenance, or any unrelated query contribution.
        state.keyboard_activation = KeyboardActivationState::Absent;
        R::Retired
    }
}
