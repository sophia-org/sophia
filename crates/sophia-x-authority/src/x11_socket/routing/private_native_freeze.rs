// A frozen request depends on exact source activations and the registrations
// that owned them. It has no decided output and no replacement origin.

pub(super) struct Freeze {
    origin: Arc<Origin>,
    keyboard: bool,
    source: crate::OrderedFreezeWitness,
    contributors: [Option<PrivateEndpointIdentity>; 2],
}

#[expect(clippy::large_enum_variant, reason = "Freeze custody is inline in storage reserved before producer exposure; boxing would allocate under native guards.")]
pub(super) enum FreezeCheck {
    Ready,
    Frozen(Freeze),
}

fn check_freeze(
    origin: &Arc<Origin>,
    authority: &crate::XInputAuthorityState,
    bindings: &PrivateAdmissionBindings,
    clients: &MutexGuard<'_, BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>>,
    previous: Option<&Freeze>,
    keyboard: bool,
) -> Result<FreezeCheck, Refusal> {
    let source = if let Some(previous) = previous {
        if !Arc::ptr_eq(origin, &previous.origin) || previous.keyboard != keyboard {
            return Err(Refusal::FreezeInvalidated);
        }
        previous.source
    } else {
        let observation = if keyboard {
            authority.ordered_keyboard_freeze(origin.namespace)
        } else {
            authority.ordered_pointer_freeze(origin.namespace)
        };
        match observation {
            crate::OrderedFreezeObservation::Ready => return Ok(FreezeCheck::Ready),
            crate::OrderedFreezeObservation::Frozen(source) => source,
            crate::OrderedFreezeObservation::Unavailable => return Err(Refusal::FreezeUnavailable),
        }
    };
    let mut contributors = [None, None];
    for (index, owner) in source.owners().into_iter().enumerate() {
        let Some(owner) = owner else { continue };
        let client = XServerFrontendClientId::from_raw(owner);
        let bound = bindings.bound.get(&client).ok_or(Refusal::FreezeInvalidated)?;
        let current = origin.registry.applied_client(clients, client, bound)
            .map_err(|_| Refusal::FreezeInvalidated)?;
        // Readmission is refused while the old lifecycle record still owes
        // source cleanup. Requiring this exact open gate also makes capture
        // meaningful when numeric client/admission names are later reused.
        if current.endpoint.lifecycle.as_ref().is_none_or(|gate| !gate.is_open()) {
            return Err(Refusal::FreezeUnavailable);
        }
        if let Some(previous) = previous
            && previous.contributors[index].as_ref().is_none_or(|old| !old.matches(&current.endpoint))
        {
            return Err(Refusal::FreezeInvalidated);
        }
        contributors[index] = Some(current.endpoint);
    }
    match authority.check_ordered_freeze(&source) {
        crate::OrderedFreezeProgress::Frozen => Ok(FreezeCheck::Frozen(Freeze {
            origin: Arc::clone(origin), keyboard, source, contributors,
        })),
        crate::OrderedFreezeProgress::Thawed if previous.is_some() => Ok(FreezeCheck::Ready),
        crate::OrderedFreezeProgress::Thawed | crate::OrderedFreezeProgress::Invalidated => {
            Err(Refusal::FreezeInvalidated)
        }
    }
}

impl BaseGuards<'_> {
    pub(super) fn freeze(
        &self,
        bindings: &PrivateAdmissionBindings,
        clients: &MutexGuard<'_, BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>>,
        previous: Option<&Freeze>,
        keyboard: bool,
    ) -> Result<FreezeCheck, Refusal> {
        check_freeze(self.origin, &self.authority, bindings, clients, previous, keyboard)
    }
}

impl Guards<'_> {
    pub(super) fn freeze(
        &self,
        bindings: &PrivateAdmissionBindings,
        clients: &MutexGuard<'_, BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>>,
        previous: Option<&Freeze>,
        keyboard: bool,
    ) -> Result<FreezeCheck, Refusal> {
        check_freeze(self.origin, &self.authority, bindings, clients, previous, keyboard)
    }
}
