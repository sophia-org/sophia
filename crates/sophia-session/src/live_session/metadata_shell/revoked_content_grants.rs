const CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RevokedContentGrantSettlement {
    pub(crate) grants: usize,
    pub(crate) claims: usize,
    pub(crate) retained: usize,
}

/// Fixed teardown inventory. Recording, retaining, and settling a revoked
/// grant never allocates, and an entry remains owned until its exact cleanup
/// action returns successfully.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RevokedContentGrantLedger {
    entries: [Option<sophia_protocol::ContentGrant>; CAPACITY],
}

impl RevokedContentGrantLedger {
    pub(crate) fn record(
        &mut self,
        grant: sophia_protocol::ContentGrant,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.entries.contains(&Some(grant)) {
            return Ok(());
        }
        let Some(slot) = self.entries.iter_mut().find(|entry| entry.is_none()) else {
            return Err("metadata shell revoked content grant ledger is full".into());
        };
        *slot = Some(grant);
        Ok(())
    }

    pub(crate) fn settle_with<E>(
        &mut self,
        mut revoke: Option<&mut dyn FnMut(sophia_protocol::ContentGrant) -> Result<usize, E>>,
    ) -> Result<RevokedContentGrantSettlement, E> {
        let Some(revoke) = revoke.as_mut() else {
            return Ok(RevokedContentGrantSettlement {
                retained: self.len(),
                ..RevokedContentGrantSettlement::default()
            });
        };
        let mut settlement = RevokedContentGrantSettlement::default();
        for entry in &mut self.entries {
            let Some(grant) = *entry else { continue };
            let claims = revoke(grant)?;
            *entry = None;
            settlement.grants += 1;
            settlement.claims += claims;
        }
        settlement.retained = self.len();
        Ok(settlement)
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }
}
