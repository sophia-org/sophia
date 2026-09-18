// Borrow one actual source; no replacement obligation or copied proof.
#[cfg(unix)]
#[derive(Clone, Copy)]
enum PrivateTerminalSource<'a> {
    Pointer(&'a private_native::Hold),
    Key(&'a private_native::KeyHold),
}

#[cfg(unix)]
impl<'a> PrivateTerminalSource<'a> {
    fn from_hold(hold: &'a PrivateNativeHold) -> Self {
        match hold {
            PrivateNativeHold::Pointer(hold) => Self::Pointer(hold),
            PrivateNativeHold::Key(hold) => Self::Key(hold),
        }
    }
    fn proof(self) -> Option<&'a private_native::Proof> {
        match self {
            Self::Pointer(hold) => hold.proof(),
            Self::Key(hold) => hold.proof(),
        }
    }
    fn has_retirement(self) -> bool {
        match self {
            Self::Pointer(hold) => hold.activation_retirement().is_some(),
            Self::Key(hold) => hold.activation_retirement().is_some(),
        }
    }
    fn depends_on(self, donor: Self) -> bool {
        match (self, donor) {
            (Self::Pointer(target), Self::Pointer(donor)) => target.needs_retirement_from(donor),
            (Self::Key(target), Self::Key(donor)) => target.needs_retirement_from(donor),
            _ => false,
        }
    }
}

#[cfg(unix)]
impl PrivateTerminalDriveCursor {
    fn next_recipient(&mut self, next: usize) {
        self.recipient = next;
        self.custody = 0;
        self.disposal_scan = 0;
        self.disposal_missing = false;
        self.disposal_ready = false;
    }
}

#[cfg(unix)]
impl PrivateTerminalInventory {
    fn terminal_source(&self, index: usize) -> Option<PrivateTerminalSource<'_>> {
        if index < self.holds.len() {
            self.holds[index]
                .native
                .as_ref()
                .map(PrivateTerminalSource::from_hold)
        } else if index < self.holds.len() + self.settling.len() {
            self.settling[index - self.holds.len()]
                .native
                .as_ref()
                .map(PrivateTerminalSource::from_hold)
        } else {
            match &self.native_pending {
                PrivateNativePending::Pointer(hold) => {
                    hold.as_ref().map(PrivateTerminalSource::Pointer)
                }
                PrivateNativePending::Key(hold) => hold.as_ref().map(PrivateTerminalSource::Key),
            }
        }
    }

    /// One exact source pair per visit. An unrelated residual cannot hold a
    /// settled record hostage; an unreadable/matching dependent keeps this
    /// donor and is retried fairly after other candidates receive their turn.
    fn native_disposal_ready(&self, cursor: &mut PrivateTerminalDriveCursor) -> bool {
        let count = self.holds.len() + self.settling.len() + 1;
        let candidate = cursor.recipient % count;
        let Some(donor) = self
            .terminal_source(candidate)
            .filter(|source| source.proof().is_some())
        else {
            cursor.next_recipient((candidate + 1) % count);
            return false;
        };
        if cursor.disposal_ready || !donor.has_retirement() {
            return true;
        }
        let index = cursor.disposal_scan % count;
        let depends = index != candidate
            && match self.terminal_source(index) {
                Some(target) => target.depends_on(donor),
                // The empty installation slot owns nothing. A record with missing
                // source custody cannot establish that it released a dependency.
                None => index < count - 1,
            };
        cursor.disposal_missing |= depends;
        cursor.disposal_scan = (index + 1) % count;
        if cursor.disposal_scan == 0 {
            if cursor.disposal_missing {
                cursor.next_recipient((candidate + 1) % count);
            } else {
                cursor.disposal_ready = true;
            }
        }
        cursor.disposal_ready
    }
}
