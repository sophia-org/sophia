//! A small recipient-transport attempt pool schedules over retained debt.
//! Native reconciliation finishes synchronously under the common guard first.
//! Reserving a task does not clear debt. Nor does a second clearing receipt let
//! an older writer pass a later press: its attempt must finish first.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttemptToken {
    authority: AuthorityUid,
    slot: usize,
    incarnation: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct AttemptRecord {
    token: AttemptToken,
    hold: HoldIncarnation,
    record_index: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct AttemptClaim {
    pub token: AttemptToken,
    pub hold: HoldIncarnation,
    pub settlement: SettlementBit,
}

impl AuthorityInstance {
    /// Reserve one attempt and advance a persistent fair cursor. Full capacity
    /// refuses without changing any hold or cursor; waiting debt stays retained.
    /// The runtime applies its time/event budget before calling this method.
    pub fn claim_next_attempt(
        &mut self,
        issuer: &IssuerHandle,
        cursor: &mut usize,
    ) -> Result<Option<AttemptClaim>, RegistrationError> {
        self.check_issuer(issuer)?;
        let slot = self
            .attempts
            .iter()
            .position(Option::is_none)
            .ok_or(RegistrationError::Capacity(CapacityError::NoAttemptSlot))?;
        let count = self.records.len();
        for _ in 0..count {
            let index = *cursor % count;
            *cursor = (index + 1) % count;
            let Some(record) = self.records[index] else {
                continue;
            };
            if record.holders != 0
                || record.attempt.is_some()
                || !record.settlement.native_reconciled
                || record.settlement.is_settled()
            {
                continue;
            }
            let token = AttemptToken {
                authority: self.uid,
                slot,
                incarnation: self.identity()?,
            };
            self.attempts[slot] = Some(AttemptRecord {
                token,
                hold: record.incarnation,
                record_index: index,
            });
            self.records[index].as_mut().expect("retained debt").attempt = Some(token);
            return Ok(Some(AttemptClaim {
                token,
                hold: record.incarnation,
                settlement: record.settlement,
            }));
        }
        Ok(None)
    }

    /// Record an actual terminal attempt outcome. The runtime must not call this
    /// on a cancellation request while the writer may still send bytes. Failed
    /// delivery finishes the attempt with neither bit set and retains the debt
    /// for a fair retry. Termination proves recipient settlement only.
    pub fn finish_attempt(
        &mut self,
        issuer: &IssuerHandle,
        token: AttemptToken,
        settlement: SettlementBit,
    ) -> Result<bool, RegistrationError> {
        self.check_issuer(issuer)?;
        if token.authority != self.uid {
            return Err(RegistrationError::ForeignAuthority);
        }
        let Some(attempt) = self
            .attempts
            .get(token.slot)
            .copied()
            .flatten()
            .filter(|entry| entry.token == token)
        else {
            return Ok(false);
        };
        let record = self.records[attempt.record_index]
            .as_mut()
            .expect("attempt pins record");
        assert_eq!(record.incarnation, attempt.hold);
        assert_eq!(record.attempt, Some(token));
        record.attempt = None;
        record.settlement.native_reconciled |= settlement.native_reconciled;
        record.settlement.recipient_settled |= settlement.recipient_settled;
        let settled = record.settlement.is_settled();
        self.attempts[token.slot] = None;
        if settled {
            self.free_record(attempt.record_index);
        }
        Ok(settled)
    }
}
