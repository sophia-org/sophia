//! Transport reservation and client knowledge, with no admission implementation.
use crate::capture::{Captured, Identity};
use sophia_9p::Errno;

pub const EBUSY: Errno = Errno(16);
pub const EALREADY: Errno = Errno(114);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Accepted { applied_ack: u64 },
    Refused(Errno),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Receipt {
    pub identity: Identity,
    pub outcome: Outcome,
}

#[derive(Default)]
pub struct Slot {
    last_sequence: u64,
    pending: Option<Receipt>,
    closed: bool,
}
impl Slot {
    /// Called only after consuming a new sequence hint. A stale cell under a
    /// new hint is an error, not idle. Failure closes the control endpoint.
    pub fn capture_published(
        &mut self,
        hint: u64,
        source: &crate::capture::Source,
        buffer: &mut Captured,
        plan: crate::capture::ReadPlan,
    ) -> std::io::Result<()> {
        if self.closed {
            return Err(std::io::Error::other("carrier closed"));
        }
        let result = if self.last_sequence.checked_add(1) != Some(hint) {
            Err(std::io::Error::other("unexpected publication hint"))
        } else {
            source
                .scan(self.last_sequence, buffer, plan)
                .and_then(|fresh| {
                    if fresh && buffer.identity.sequence == hint {
                        Ok(())
                    } else {
                        Err(std::io::Error::other(
                            "cell does not match publication hint",
                        ))
                    }
                })
        };
        if result.is_err() {
            self.close();
        }
        result
    }
    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn close(&mut self) {
        self.closed = true;
        self.pending = None;
    }
    /// The adapter owns the fid, fresh checks, ACK effects and all admission.
    /// Reserve before calling it; a transport failure never undoes its effects.
    pub fn process(
        &mut self,
        request: &Captured,
        dispatch: impl FnOnce(&[u8]) -> Result<u32, Errno>,
    ) -> Result<(), Errno> {
        if self.closed {
            return Err(Errno::ESTALE);
        }
        if self.pending.is_some() {
            return Err(EBUSY);
        }
        let id = request.identity;
        if id.sequence == u64::MAX || self.last_sequence.checked_add(1) != Some(id.sequence) {
            self.close();
            return Err(Errno::EINVAL);
        }
        let bytes = &request.bytes;
        if bytes.len() < 32
            || bytes.len() as u64 != id.bytes
            || bytes[8..16] != id.epoch.to_le_bytes()
            || bytes[16..24] != id.submission.to_le_bytes()
            || bytes[24..32] != id.ack.to_le_bytes()
        {
            self.close();
            return Err(Errno::EINVAL);
        }
        self.last_sequence = id.sequence;
        self.pending = Some(Receipt {
            identity: id,
            outcome: Outcome::Refused(Errno::EINVAL),
        });
        let outcome = match dispatch(bytes) {
            Ok(count) if count as u64 == id.bytes => Outcome::Accepted {
                applied_ack: id.ack,
            },
            Ok(_) => {
                self.close();
                return Err(Errno::EINVAL);
            }
            Err(error) => Outcome::Refused(error),
        };
        self.pending.as_mut().expect("reserved receipt").outcome = outcome;
        Ok(())
    }
    /// The caller rechecks delivery authority; no retained live flag grants it.
    pub fn publish(&mut self, authority_live: bool) -> Option<Receipt> {
        if self.closed || !authority_live {
            self.close();
            return None;
        }
        self.pending.take()
    }
}

#[derive(Default)]
pub struct Client {
    pending: Option<Identity>,
    pub known: Option<u64>,
    pub confirmed_ack: u64,
    pub unknown: bool,
    pub closed: bool,
    binding: Option<(u64, u64)>,
    needs_reopen: bool,
    standalone_ack: Option<u64>,
}
impl Client {
    /// SQ bytes must remain immutable until this attempt's receipt is observed.
    pub fn begin(&mut self, id: Identity) -> Result<(), Errno> {
        if self.closed || self.unknown {
            return Err(Errno::ESTALE);
        }
        if self.pending.is_some() || self.standalone_ack.is_some() {
            return Err(EBUSY);
        }
        if self.needs_reopen || self.binding.is_some_and(|b| b != (id.epoch, id.writer)) {
            return Err(Errno::ESTALE);
        }
        self.binding = Some((id.epoch, id.writer));
        self.pending = Some(id);
        Ok(())
    }
    pub fn reopen_writer(&mut self, writer: u64) -> Result<(), Errno> {
        if self.closed || self.unknown || self.pending.is_some() || !self.needs_reopen {
            return Err(Errno::ESTALE);
        }
        let (epoch, old) = self.binding.ok_or(Errno::ESTALE)?;
        if writer <= old {
            return Err(Errno::ESTALE);
        }
        self.binding = Some((epoch, writer));
        self.needs_reopen = false;
        Ok(())
    }
    pub fn standalone_ack_allowed(&self) -> bool {
        !self.closed && !self.unknown && self.pending.is_none() && self.standalone_ack.is_none()
    }
    pub fn begin_ack(&mut self, word: u64) -> Result<(), Errno> {
        if !self.standalone_ack_allowed() {
            return Err(EBUSY);
        }
        self.standalone_ack = Some(word);
        Ok(())
    }
    pub fn finish_ack(&mut self, word: u64) -> Result<(), Errno> {
        if self.closed || self.standalone_ack != Some(word) {
            self.disconnect();
            return Err(Errno::EINVAL);
        }
        self.standalone_ack = None;
        self.confirmed_ack = self.confirmed_ack.max(word);
        Ok(())
    }
    pub fn disconnect(&mut self) {
        self.unknown |= self.pending.is_some();
        self.closed = true;
    }
    /// Every pipe record must answer the one outstanding attempt exactly.
    pub fn observe(&mut self, receipt: Receipt) -> Result<Outcome, Errno> {
        if self.closed || self.pending != Some(receipt.identity) {
            self.disconnect();
            return Err(Errno::EINVAL);
        }
        if let Outcome::Accepted { applied_ack } = receipt.outcome {
            if applied_ack != receipt.identity.ack {
                self.disconnect();
                return Err(Errno::EINVAL);
            }
            self.known = Some(receipt.identity.submission);
            self.confirmed_ack = self.confirmed_ack.max(applied_ack);
        }
        self.pending = None;
        match receipt.outcome {
            Outcome::Refused(Errno::ESTALE) => self.needs_reopen = true,
            Outcome::Refused(Errno::EAGAIN) | Outcome::Accepted { .. } => (),
            Outcome::Refused(error) => {
                self.unknown |= error == EALREADY;
                self.closed = true;
            }
        }
        Ok(receipt.outcome)
    }
}
