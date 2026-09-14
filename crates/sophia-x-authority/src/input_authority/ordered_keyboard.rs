/// One applied keyboard activation inside an origin's native authority.
/// The native owner must also retain its exact origin: names alone cannot
/// distinguish two independent authorities. Namespace storage may disappear;
/// the serial allocator lives on the authority so recreation cannot reuse it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KeyboardActivationStamp {
    namespace: NamespaceId,
    serial: u64,
}

impl KeyboardActivationStamp {
    fn reserve(high_water: &mut u64, namespace: NamespaceId) -> Option<Self> {
        let serial = high_water.checked_add(1)?;
        *high_water = serial;
        Some(Self { namespace, serial })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum KeyboardActivationState {
    #[default]
    Absent,
    // Write ahead of the actual grab fields. An interrupted or unnameable
    // activation cannot answer an ordered reader as either applied or absent.
    Changing,
    Applied(KeyboardActivationStamp),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyboardActivationRefusal {
    NamespaceUnprepared,
    ProvenanceUnavailable,
}

/// Read-only provenance from the actual grab producers. This does not grant a
/// key, select a passive grab, mutate XKB, or establish a retirement receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KeyboardActivation {
    stamp: KeyboardActivationStamp,
    recipient: XActiveInputGrab,
    trigger: Option<u8>,
}

#[cfg_attr(not(test), allow(dead_code))] // Guarded key source integration is still required.
impl KeyboardActivation {
    pub(crate) fn stamp(self) -> KeyboardActivationStamp {
        self.stamp
    }
    pub(crate) fn recipient(self) -> XActiveInputGrab {
        self.recipient
    }
    pub(crate) fn trigger(self) -> Option<u8> {
        self.trigger
    }
}

impl XInputAuthorityState {
    /// Read under the same exclusive authority guard as final validation.
    /// Ordinary producers preserve their existing grab behavior even if the
    /// private identity allocator is exhausted; this read then fails closed.
    /// Absence is a current observation, never proof that an older hold's
    /// native contributions were retired.
    #[cfg_attr(not(test), allow(dead_code))] // No private keys are enabled by a provenance accessor.
    pub(crate) fn keyboard_activation(
        &self,
        namespace: NamespaceId,
    ) -> Result<Option<KeyboardActivation>, KeyboardActivationRefusal> {
        let state = self
            .namespaces
            .get(&namespace)
            .ok_or(KeyboardActivationRefusal::NamespaceUnprepared)?;
        match (state.keyboard_activation, state.keyboard) {
            (KeyboardActivationState::Absent, None) => Ok(None),
            (KeyboardActivationState::Applied(stamp), Some(recipient))
                if stamp.namespace == namespace =>
            {
                Ok(Some(KeyboardActivation {
                    stamp,
                    recipient,
                    trigger: state.keyboard_passive_detail,
                }))
            }
            _ => Err(KeyboardActivationRefusal::ProvenanceUnavailable),
        }
    }
}
