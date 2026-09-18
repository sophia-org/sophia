const FREEZE_POINTER: u8 = 1;
const FREEZE_KEYBOARD: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OrderedFreezeSource {
    Pointer(PointerActivationStamp),
    Keyboard(KeyboardActivationStamp),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FreezeContribution {
    // Ordinary protocol grabs may exhaust the ordered identity allocator.
    // Their freeze remains effective, but cannot issue an ordered witness.
    source: Option<OrderedFreezeSource>,
    owner: u64,
    pending: u8,
    asynchronous: u8,
}

impl FreezeContribution {
    fn new(source: Option<OrderedFreezeSource>, grab: XActiveInputGrab) -> Self {
        Self {
            source,
            owner: grab.owner,
            pending: (u8::from(grab.pointer_mode == 0) * FREEZE_POINTER)
                | (u8::from(grab.keyboard_mode == 0) * FREEZE_KEYBOARD),
            asynchronous: 0,
        }
    }
}

/// At most one active pointer grab and one active keyboard grab can freeze
/// either device. Keep both contributions, including a cleared mask, until
/// their activation ends; only an exact surviving activation can prove thaw.
#[derive(Clone, Debug, Default)]
struct OrderedFreezeState {
    pointer: Option<FreezeContribution>,
    keyboard: Option<FreezeContribution>,
}

impl OrderedFreezeState {
    fn activate_pointer(&mut self, stamp: Option<PointerActivationStamp>, grab: XActiveInputGrab) {
        self.pointer = Some(FreezeContribution::new(
            stamp.map(OrderedFreezeSource::Pointer),
            grab,
        ));
    }

    fn activate_keyboard(
        &mut self,
        stamp: Option<KeyboardActivationStamp>,
        grab: XActiveInputGrab,
    ) {
        self.keyboard = Some(FreezeContribution::new(
            stamp.map(OrderedFreezeSource::Keyboard),
            grab,
        ));
    }

    fn frozen(&self, device: u8) -> bool {
        [self.pointer, self.keyboard]
            .into_iter()
            .flatten()
            .any(|contribution| contribution.pending & device != 0)
    }

    fn allow_events(&mut self, owner: u64, mode: u8) {
        let devices = match mode {
            0..=2 => FREEZE_POINTER,
            3..=5 => FREEZE_KEYBOARD,
            6 | 7 => FREEZE_POINTER | FREEZE_KEYBOARD,
            _ => return,
        };
        for contribution in [&mut self.pointer, &mut self.keyboard]
            .into_iter()
            .flatten()
        {
            if contribution.owner != owner {
                continue;
            }
            let released = contribution.pending & devices;
            contribution.pending &= !devices;
            // Existing public handling clears the requested device for all
            // AllowEvents modes. Only AsyncPointer/Keyboard/Both supplies the
            // private ordered path's persistent thaw evidence. Replay and
            // one-event Sync require separate native continuations.
            if matches!(mode, 0 | 3 | 6) {
                contribution.asynchronous |= released;
            }
        }
    }

    fn remove_owner(&mut self, owner: u64) -> OrderedOwnerFreezeReceipt {
        let mut receipt = OrderedOwnerFreezeReceipt {
            pointer: None,
            keyboard: None,
        };
        if self
            .pointer
            .is_some_and(|contribution| contribution.owner == owner)
            && let Some(contribution) = self.pointer.take()
            && let Some(OrderedFreezeSource::Pointer(stamp)) = contribution.source
        {
            receipt.pointer = Some(stamp);
        }
        if self
            .keyboard
            .is_some_and(|contribution| contribution.owner == owner)
            && let Some(contribution) = self.keyboard.take()
            && let Some(OrderedFreezeSource::Keyboard(stamp)) = contribution.source
        {
            receipt.keyboard = Some(stamp);
        }
        receipt
    }

    fn observe(&self, namespace: NamespaceId, device: u8) -> OrderedFreezeObservation {
        let mut contributors = [None, None];
        for (index, contribution) in [self.pointer, self.keyboard].into_iter().enumerate() {
            let Some(contribution) = contribution.filter(|value| value.pending & device != 0)
            else {
                continue;
            };
            let Some(source) = contribution.source else {
                return OrderedFreezeObservation::Unavailable;
            };
            contributors[index] = Some((source, contribution.owner));
        }
        if contributors == [None, None] {
            OrderedFreezeObservation::Ready
        } else {
            OrderedFreezeObservation::Frozen(OrderedFreezeWitness {
                namespace,
                device,
                contributors,
            })
        }
    }

    fn check(&self, witness: &OrderedFreezeWitness) -> OrderedFreezeProgress {
        let mut frozen = false;
        for (previous, current) in witness
            .contributors
            .into_iter()
            .zip([self.pointer, self.keyboard])
        {
            let Some((source, owner)) = previous else {
                if current.is_some_and(|value| value.pending & witness.device != 0) {
                    return OrderedFreezeProgress::Invalidated;
                }
                continue;
            };
            let Some(current) = current else {
                return OrderedFreezeProgress::Invalidated;
            };
            if current.source != Some(source) || current.owner != owner {
                return OrderedFreezeProgress::Invalidated;
            }
            if current.pending & witness.device != 0 {
                frozen = true;
            } else if current.asynchronous & witness.device == 0 {
                return OrderedFreezeProgress::Invalidated;
            }
        }
        if frozen {
            OrderedFreezeProgress::Frozen
        } else {
            OrderedFreezeProgress::Thawed
        }
    }
}

/// Source-produced dependency on the exact activations freezing one device.
/// The retaining private owner must also bind the original native authority,
/// connection/admission and seat; activation names are local to that authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OrderedFreezeWitness {
    namespace: NamespaceId,
    device: u8,
    contributors: [Option<(OrderedFreezeSource, u64)>; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OrderedFreezeObservation {
    Ready,
    Frozen(OrderedFreezeWitness),
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OrderedFreezeProgress {
    Frozen,
    Thawed,
    Invalidated,
}

/// Answers only exact activation contributions removed by the issuing call.
/// Other owners may still freeze either device; global booleans are not proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OrderedOwnerFreezeReceipt {
    pointer: Option<PointerActivationStamp>,
    keyboard: Option<KeyboardActivationStamp>,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "The stopped native owner will consume exact freeze removal receipts."
    )
)]
impl OrderedOwnerFreezeReceipt {
    pub(crate) fn answers_pointer(&self, stamp: PointerActivationStamp) -> bool {
        self.pointer == Some(stamp)
    }

    pub(crate) fn answers_keyboard(&self, stamp: KeyboardActivationStamp) -> bool {
        self.keyboard == Some(stamp)
    }
}

impl XInputAuthorityState {
    /// Bounded inspection under the caller's retained native authority guard;
    /// neither method allocates or selects an event recipient.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "The retained private input owner will consume source freeze witnesses."
        )
    )]
    pub(crate) fn ordered_pointer_freeze(
        &self,
        namespace: NamespaceId,
    ) -> OrderedFreezeObservation {
        self.ordered_freeze(namespace, FREEZE_POINTER)
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "The retained private input owner will consume source freeze witnesses."
        )
    )]
    pub(crate) fn ordered_keyboard_freeze(
        &self,
        namespace: NamespaceId,
    ) -> OrderedFreezeObservation {
        self.ordered_freeze(namespace, FREEZE_KEYBOARD)
    }

    fn ordered_freeze(&self, namespace: NamespaceId, device: u8) -> OrderedFreezeObservation {
        self.namespaces
            .get(&namespace)
            .map_or(OrderedFreezeObservation::Unavailable, |state| {
                if state.pointer_activation == PointerActivationState::Changing
                    || state.keyboard_activation == KeyboardActivationState::Changing
                {
                    return OrderedFreezeObservation::Unavailable;
                }
                state.freeze.observe(namespace, device)
            })
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "The retained private input owner will consume source freeze witnesses."
        )
    )]
    pub(crate) fn check_ordered_freeze(
        &self,
        witness: &OrderedFreezeWitness,
    ) -> OrderedFreezeProgress {
        self.namespaces.get(&witness.namespace).map_or(
            OrderedFreezeProgress::Invalidated,
            |state| {
                if state.pointer_activation == PointerActivationState::Changing
                    || state.keyboard_activation == KeyboardActivationState::Changing
                {
                    return OrderedFreezeProgress::Invalidated;
                }
                state.freeze.check(witness)
            },
        )
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "The stopped native owner will consume exact freeze removal receipts."
        )
    )]
    pub(crate) fn cleanup_ordered_freeze_owner(
        &mut self,
        namespace: NamespaceId,
        owner: u64,
    ) -> OrderedOwnerFreezeReceipt {
        self.namespaces.get_mut(&namespace).map_or(
            OrderedOwnerFreezeReceipt {
                pointer: None,
                keyboard: None,
            },
            |state| state.freeze.remove_owner(owner),
        )
    }
}
