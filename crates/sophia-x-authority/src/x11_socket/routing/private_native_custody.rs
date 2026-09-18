/// One source obligation, carried whole through held and settling storage.
/// The variants share custody operations, never constructors for native proof.
#[cfg(unix)]
#[expect(
    clippy::large_enum_variant,
    reason = "inline storage is reserved before producer exposure"
)]
enum PrivateNativeHold {
    Pointer(private_native::Hold),
    Key(private_native::KeyHold),
}

#[cfg(unix)]
impl PrivateNativeHold {
    fn input(&self) -> sophia_input_authority::Input {
        match self {
            Self::Pointer(hold) => hold.input(),
            Self::Key(hold) => hold.input(),
        }
    }

    fn client(&self) -> XServerFrontendClientId {
        match self {
            Self::Pointer(hold) => hold.client(),
            Self::Key(hold) => hold.client(),
        }
    }

    fn connection(&self) -> private_native::RetainedConnection {
        match self {
            Self::Pointer(hold) => hold.connection(),
            Self::Key(hold) => hold.connection(),
        }
    }

    fn incarnation(&self) -> Option<sophia_input_authority::HoldIncarnation> {
        match self {
            Self::Pointer(hold) => hold.incarnation(),
            Self::Key(hold) => hold.incarnation(),
        }
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "custody controls inspect the retained source phase"
        )
    )]
    fn status(&self) -> private_native::Status {
        match self {
            Self::Pointer(hold) => hold.status(),
            Self::Key(hold) => hold.status(),
        }
    }

    fn proof(&self) -> Option<&private_native::Proof> {
        match self {
            Self::Pointer(hold) => hold.proof(),
            Self::Key(hold) => hold.proof(),
        }
    }

    fn take_press_emission(&mut self) -> Option<PrivateOrderedEmission> {
        match self {
            Self::Pointer(hold) => hold.take_press_emission(),
            Self::Key(hold) => hold.take_press_emission(),
        }
    }

    fn take_release_emission(&mut self) -> Option<PrivateOrderedEmission> {
        match self {
            Self::Pointer(hold) => hold.take_release_emission(),
            Self::Key(hold) => hold.take_release_emission(),
        }
    }

    fn pointer(&self) -> Option<&private_native::Hold> {
        match self {
            Self::Pointer(hold) => Some(hold),
            Self::Key(_) => None,
        }
    }

    fn pointer_mut(&mut self) -> Option<&mut private_native::Hold> {
        match self {
            Self::Pointer(hold) => Some(hold),
            Self::Key(_) => None,
        }
    }

    fn key(&self) -> Option<&private_native::KeyHold> {
        match self {
            Self::Pointer(_) => None,
            Self::Key(hold) => Some(hold),
        }
    }

    fn key_mut(&mut self) -> Option<&mut private_native::KeyHold> {
        match self {
            Self::Pointer(_) => None,
            Self::Key(hold) => Some(hold),
        }
    }
}

/// Source-owned installation slots already inside the terminal inventory.
/// A source borrows its typed slot before applying; an interruption leaves
/// whatever it installed here. A populated slot can never change kind.
#[cfg(unix)]
#[expect(
    clippy::large_enum_variant,
    reason = "reserve the largest native obligation inline"
)]
enum PrivateNativePending {
    Pointer(Option<private_native::Hold>),
    Key(Option<private_native::KeyHold>),
}

#[cfg(unix)]
impl Default for PrivateNativePending {
    fn default() -> Self {
        Self::Pointer(None)
    }
}

#[cfg(unix)]
impl PrivateNativePending {
    fn is_none(&self) -> bool {
        match self {
            Self::Pointer(hold) => hold.is_none(),
            Self::Key(hold) => hold.is_none(),
        }
    }

    fn is_some(&self) -> bool {
        !self.is_none()
    }

    fn pointer_slot(
        &mut self,
    ) -> Result<&mut Option<private_native::Hold>, private_native::Refusal> {
        if matches!(self, Self::Key(Some(_))) {
            return Err(private_native::Refusal::WrongPhase);
        }
        if matches!(self, Self::Key(None)) {
            *self = Self::Pointer(None);
        }
        match self {
            Self::Pointer(slot) => Ok(slot),
            Self::Key(_) => unreachable!("a populated key slot was refused"),
        }
    }

    fn key_slot(
        &mut self,
    ) -> Result<&mut Option<private_native::KeyHold>, private_native::Refusal> {
        if matches!(self, Self::Pointer(Some(_))) {
            return Err(private_native::Refusal::WrongPhase);
        }
        if matches!(self, Self::Pointer(None)) {
            *self = Self::Key(None);
        }
        match self {
            Self::Key(slot) => Ok(slot),
            Self::Pointer(_) => unreachable!("a populated pointer slot was refused"),
        }
    }

    fn pointer(&self) -> Option<&private_native::Hold> {
        match self {
            Self::Pointer(hold) => hold.as_ref(),
            Self::Key(_) => None,
        }
    }

    fn key(&self) -> Option<&private_native::KeyHold> {
        match self {
            Self::Pointer(_) => None,
            Self::Key(hold) => hold.as_ref(),
        }
    }

    /// Only move after the destination record exists. This neither clones
    /// native state nor resolves anything from the current registry.
    fn take(&mut self) -> Option<PrivateNativeHold> {
        match self {
            Self::Pointer(hold) => hold.take().map(PrivateNativeHold::Pointer),
            Self::Key(hold) => hold.take().map(PrivateNativeHold::Key),
        }
    }
}
