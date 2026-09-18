//! Issuer-only native cleanup of one exact zero-holder incarnation.
use super::*;

/// The original authority is exclusively borrowed until native cleanup ends.
/// This grants no production input, changes no hold and proves no native or
/// recipient effect. Only a retained debt owned by the named participant can
/// obtain it, after every common holder has gone.
pub struct NativeReconciliationPermit<'a> {
    authority: &'a mut AuthorityInstance,
    incarnation: HoldIncarnation,
    owner: Option<GrantId>,
}

impl NativeReconciliationPermit<'_> {
    pub fn identity(&self) -> AuthorityIdentity {
        AuthorityIdentity(self.authority.uid)
    }
    pub fn incarnation(&self) -> HoldIncarnation {
        self.incarnation
    }
    pub fn owner(&self) -> Option<GrantId> {
        self.owner
    }
}

impl AuthorityInstance {
    /// Read one exact issuer-owned record after independently obtained source
    /// and recipient evidence has been recorded. Absence supplies no evidence.
    pub fn reconciliation_record_present(
        &self,
        issuer: &IssuerHandle,
        incarnation: HoldIncarnation,
    ) -> Result<bool, RegistrationError> {
        self.check_issuer(issuer)?;
        if incarnation.authority != self.uid {
            return Err(RegistrationError::ForeignAuthority);
        }
        Ok(self
            .records
            .iter()
            .flatten()
            .any(|record| record.incarnation == incarnation))
    }

    pub fn native_reconciliation(
        &mut self,
        issuer: &IssuerHandle,
        owner: Option<GrantId>,
        incarnation: HoldIncarnation,
    ) -> Result<NativeReconciliationPermit<'_>, RegistrationError> {
        self.check_issuer(issuer)?;
        if incarnation.authority != self.uid {
            return Err(RegistrationError::ForeignAuthority);
        }
        let record = self
            .records
            .iter()
            .flatten()
            .find(|record| record.incarnation == incarnation)
            .ok_or(RegistrationError::StaleGeneration)?;
        if record.holders != 0 {
            return Err(RegistrationError::ReleaseBarrier);
        }
        let authorized = (0..u64::BITS as usize).any(|bit| {
            record.participants & (1u64 << bit) != 0
                && self
                    .record(self.source_at_bit(bit))
                    .is_some_and(|source| source.owner == owner)
        });
        if !authorized {
            return Err(RegistrationError::StaleGeneration);
        }
        Ok(NativeReconciliationPermit {
            authority: self,
            incarnation,
            owner,
        })
    }
}
