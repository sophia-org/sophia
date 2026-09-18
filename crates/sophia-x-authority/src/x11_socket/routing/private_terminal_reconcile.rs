#[cfg(unix)]
impl PrivateNativeHold {
    fn endpoint(&self) -> &PrivateEndpointIdentity {
        match self {
            Self::Pointer(hold) => hold.endpoint(),
            Self::Key(hold) => hold.endpoint(),
        }
    }
    fn grant(&self) -> sophia_input_authority::GrantId {
        match self {
            Self::Pointer(hold) => hold.grant(),
            Self::Key(hold) => hold.grant(),
        }
    }
    fn reconcile(
        &mut self,
        controller: &PrivateAuthorityController,
        owner: &private_native::Owner,
        keyboards: &mut PrivateKeyboards,
    ) -> Result<bool, PrivateTerminalDriveRefusal> {
        use PrivateTerminalDriveRefusal as Refusal;
        if self.proof().is_some() {
            return Ok(false);
        }
        let incarnation = self.incarnation().ok_or(Refusal::MissingIncarnation)?;
        let grant = self.grant();
        let connection = self.connection();
        controller
            .under_common_as_origin(|authority, issuer| {
                let permit = authority
                    .native_reconciliation(issuer, Some(grant), incarnation)
                    .map_err(|cause| Refusal::Common(PrivateAuthorityRefusal::Authority(cause)))?;
                let mut guards = owner
                    .lock_for_release(&connection)
                    .map_err(Refusal::Native)?;
                match self {
                    Self::Pointer(hold) => guards.reconcile_pointer(&permit, hold),
                    Self::Key(hold) => guards.reconcile_key(&permit, hold, keyboards),
                }
                .map_err(Refusal::Native)
            })
            .map_err(Refusal::Common)?
    }
}

#[cfg(unix)]
impl PrivateTerminalInventory {
    fn reconcile_native_one(
        &mut self,
        owner: &private_native::Owner,
        keyboards: &mut PrivateKeyboards,
        cursor: &mut usize,
    ) -> Result<PrivateTerminalVisit, PrivateTerminalDriveRefusal> {
        use PrivateTerminalDriveRefusal as Refusal;
        let count = self.holds.len() + self.settling.len() + 1;
        let index = *cursor % count;
        *cursor = (index + 1) % count;
        let reconciled = if index < self.holds.len() {
            self.holds[index]
                .native
                .as_mut()
                .ok_or(Refusal::MissingNative)?
                .reconcile(&self.controller, owner, keyboards)?
        } else if index < count - 1 {
            self.settling[index - self.holds.len()]
                .native
                .as_mut()
                .ok_or(Refusal::MissingNative)?
                .reconcile(&self.controller, owner, keyboards)?
        } else {
            // An interruption may have left source custody in the installed
            // slot. Borrow it in place; do not extract it before a destination.
            match &mut self.native_pending {
                PrivateNativePending::Pointer(Some(hold)) => {
                    if hold.proof().is_some() {
                        return Ok(PrivateTerminalVisit::Native { reconciled: false });
                    }
                    let incarnation = hold.incarnation().ok_or(Refusal::MissingIncarnation)?;
                    let connection = hold.connection();
                    self.controller
                        .under_common_as_origin(|authority, issuer| {
                            let permit = authority
                                .native_reconciliation(issuer, Some(hold.grant()), incarnation)
                                .map_err(|cause| {
                                    Refusal::Common(PrivateAuthorityRefusal::Authority(cause))
                                })?;
                            owner
                                .lock_for_release(&connection)
                                .map_err(Refusal::Native)?
                                .reconcile_pointer(&permit, hold)
                                .map_err(Refusal::Native)
                        })
                        .map_err(Refusal::Common)??
                }
                PrivateNativePending::Key(Some(hold)) => {
                    if hold.proof().is_some() {
                        return Ok(PrivateTerminalVisit::Native { reconciled: false });
                    }
                    let incarnation = hold.incarnation().ok_or(Refusal::MissingIncarnation)?;
                    let connection = hold.connection();
                    self.controller
                        .under_common_as_origin(|authority, issuer| {
                            let permit = authority
                                .native_reconciliation(issuer, Some(hold.grant()), incarnation)
                                .map_err(|cause| {
                                    Refusal::Common(PrivateAuthorityRefusal::Authority(cause))
                                })?;
                            owner
                                .lock_for_release(&connection)
                                .map_err(Refusal::Native)?
                                .reconcile_key(&permit, hold, keyboards)
                                .map_err(Refusal::Native)
                        })
                        .map_err(Refusal::Common)??
                }
                _ => false,
            }
        };
        if reconciled {
            self.shared_activation.invalidate();
        }
        Ok(PrivateTerminalVisit::Native { reconciled })
    }
}

#[cfg(unix)]
impl PrivateTerminalInventory {
    fn record_terminal_native_one(
        &mut self,
        cursor: &mut usize,
    ) -> Result<PrivateTerminalVisit, PrivateTerminalDriveRefusal> {
        let count = self.holds.len() + self.settling.len() + 1;
        let index = *cursor % count;
        *cursor = (index + 1) % count;
        let proof = if index < self.holds.len() {
            self.holds[index]
                .native
                .as_ref()
                .and_then(PrivateNativeHold::proof)
        } else if index < count - 1 {
            self.settling[index - self.holds.len()]
                .native
                .as_ref()
                .and_then(PrivateNativeHold::proof)
        } else {
            match &self.native_pending {
                PrivateNativePending::Pointer(hold) => {
                    hold.as_ref().and_then(private_native::Hold::proof)
                }
                PrivateNativePending::Key(hold) => {
                    hold.as_ref().and_then(private_native::KeyHold::proof)
                }
            }
        };
        let settled = match proof {
            Some(proof) => proof
                .record_native()
                .map_err(PrivateTerminalDriveRefusal::Common)?,
            None => false,
        };
        Ok(PrivateTerminalVisit::Recorded { settled })
    }
}
