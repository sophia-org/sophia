use std::collections::VecDeque;
use std::sync::Arc;

use crate::custody::{Admission, Ledger};
use crate::{MAX_QUEUED_BYTES, MAX_QUEUED_FRAMES, ShellClientError};

// The r5 ceiling is sixteen pending actions. Each can need an ACK plus an
// indicator activation. Uploads cannot occupy these record or byte credits.
const CONTROL_RECORDS: usize = 32;
const CONTROL_BYTES: usize = CONTROL_RECORDS * 256;

/// Local admission bounds, independent of server Limits. Control records may
/// use the whole queue; bulk records leave the configured headroom unused.
/// Zero headroom selects a shared pool, like the C session queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueLimits {
    pub records: usize,
    pub bytes: usize,
    pub control_records: usize,
    pub control_bytes: usize,
}
impl Default for QueueLimits {
    fn default() -> Self {
        Self {
            records: MAX_QUEUED_FRAMES,
            bytes: MAX_QUEUED_BYTES,
            control_records: CONTROL_RECORDS,
            control_bytes: CONTROL_BYTES,
        }
    }
}
impl QueueLimits {
    fn validate(self) -> Result<(), ShellClientError> {
        if self.records == 0
            || self.records > MAX_QUEUED_FRAMES
            || self.bytes == 0
            || self.bytes > MAX_QUEUED_BYTES
            || self.control_records > self.records
            || self.control_bytes > self.bytes
        {
            return Err(ShellClientError::InvalidArgument("queue limits"));
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct ClientOutbox {
    limits: QueueLimits,
    frames: VecDeque<Frame>,
    bytes: usize,
    bulk_records: usize,
    bulk_bytes: usize,
    reserved: Option<ReservedCapacity>,
}

/// Connection-bound queue credits for records chosen later. This token does
/// not borrow the connection, so dispatch and other admissions may continue.
/// Cancel explicitly if unused; dropping the token alone does not release
/// credits. A terminal connection releases them. Tokens cannot be reused or
/// transferred to another connection.
#[derive(Debug)]
#[must_use = "commit or explicitly cancel the reserved capacity"]
pub struct QueueReservation(Arc<()>);

struct ReservedCapacity {
    identity: Arc<()>,
    slots: usize,
    bytes: usize,
}
struct Frame {
    bytes: Option<Box<[u8]>>,
    /// Charges stay here while the file wire owns the payload.
    len: usize,
    control: bool,
    ticket: u64,
}
pub(crate) struct PreparedQueue {
    frames: Vec<Frame>,
    bytes: usize,
    control: bool,
}
impl PreparedQueue {
    pub(crate) fn last_bytes(&self) -> &[u8] {
        self.frames.last().unwrap().bytes.as_deref().unwrap()
    }
    /// Caller has encoded an alternative for the same record identity. Equal
    /// size preserves the reserved capacity and wire's staged record metadata.
    pub(crate) fn replace_last_same_size(&mut self, bytes: Box<[u8]>) {
        let frame = self.frames.last_mut().unwrap();
        assert_eq!(frame.len, bytes.len());
        frame.bytes = Some(bytes);
    }
}
impl ClientOutbox {
    pub(crate) fn limits(&self) -> QueueLimits {
        self.limits
    }
    pub(crate) fn set_limits(&mut self, limits: QueueLimits) -> Result<(), ShellClientError> {
        limits.validate()?;
        // The front remains charged while the wire owns its bytes, including
        // Submitted wait. Never resize across queued, in-flight or held credits.
        if !self.frames.is_empty() || self.reserved.is_some() {
            return Err(ShellClientError::QueueSaturated);
        }
        self.limits = limits;
        Ok(())
    }
    pub(crate) fn reserve_capacity(
        &mut self,
        slots: usize,
        bytes: usize,
    ) -> Result<QueueReservation, ShellClientError> {
        if slots == 0
            || bytes == 0
            || slots > self.limits.records - self.limits.control_records
            || bytes > self.limits.bytes - self.limits.control_bytes
        {
            return Err(ShellClientError::InvalidArgument("reservation size"));
        }
        if self.reserved.is_some() || !self.fits(slots, bytes, false) {
            return Err(ShellClientError::QueueSaturated);
        }
        self.frames
            .try_reserve(slots)
            .map_err(|_| ShellClientError::QueueSaturated)?;
        let identity = Arc::new(());
        let token = QueueReservation(identity.clone());
        self.reserved = Some(ReservedCapacity {
            identity,
            slots,
            bytes,
        });
        Ok(token)
    }

    pub(crate) fn validate_reservation(
        &self,
        token: &QueueReservation,
    ) -> Result<(), ShellClientError> {
        match &self.reserved {
            Some(reserved) if Arc::ptr_eq(&reserved.identity, &token.0) => Ok(()),
            _ => Err(ShellClientError::InvalidArgument("reservation identity")),
        }
    }

    pub(crate) fn cancel_reservation(
        &mut self,
        token: &QueueReservation,
    ) -> Result<(), ShellClientError> {
        self.validate_reservation(token)?;
        self.reserved = None;
        Ok(())
    }

    pub(crate) fn release_reservation(&mut self) {
        self.reserved = None;
    }

    fn fits(&self, slots: usize, bytes: usize, control: bool) -> bool {
        let (reserved_slots, reserved_bytes) = self
            .reserved
            .as_ref()
            .map_or((0, 0), |r| (r.slots, r.bytes));
        self.frames
            .len()
            .saturating_add(reserved_slots)
            .saturating_add(slots)
            <= self.limits.records
            && self
                .bytes
                .saturating_add(reserved_bytes)
                .saturating_add(bytes)
                <= self.limits.bytes
            && (control
                || (self
                    .bulk_records
                    .saturating_add(reserved_slots)
                    .saturating_add(slots)
                    <= self.limits.records - self.limits.control_records
                    && self
                        .bulk_bytes
                        .saturating_add(reserved_bytes)
                        .saturating_add(bytes)
                        <= self.limits.bytes - self.limits.control_bytes))
    }

    pub(crate) fn prepare_reserved(
        &mut self,
        token: &QueueReservation,
        frames: Vec<Vec<u8>>,
        control: bool,
        ledger: &mut Ledger,
    ) -> Result<PreparedQueue, ShellClientError> {
        self.validate_reservation(token)?;
        let reserved = self.reserved.as_ref().expect("validated reservation");
        let bytes = frames
            .iter()
            .try_fold(0usize, |sum, frame| sum.checked_add(frame.len()));
        if frames.len() > reserved.slots || bytes.is_none_or(|n| n > reserved.bytes) {
            return Err(ShellClientError::InvalidArgument(
                "records exceed reservation",
            ));
        }
        let reserved = self.reserved.take();
        match self.prepare(frames, control, ledger) {
            Ok(prepared) => Ok(prepared),
            Err(error) => {
                self.reserved = reserved;
                Err(error)
            }
        }
    }
    pub(crate) fn enqueue(
        &mut self,
        frames: Vec<Vec<u8>>,
        control: bool,
        ledger: &mut Ledger,
    ) -> Result<Admission, ShellClientError> {
        self.enqueue_after(frames, control, ledger, || Ok(()))
    }

    /// Reserve the complete FIFO transfer before updating its producer. The
    /// callback cannot access this mutably borrowed FIFO. Once it succeeds,
    /// only infallible moves and integer accounting remain before ownership.
    /// Each unit gets its ticket then, so a refused admission spends none.
    pub(crate) fn enqueue_after(
        &mut self,
        frames: Vec<Vec<u8>>,
        control: bool,
        ledger: &mut Ledger,
        before_commit: impl FnOnce() -> Result<(), ShellClientError>,
    ) -> Result<Admission, ShellClientError> {
        let prepared = self.prepare(frames, control, ledger)?;
        before_commit()?;
        Ok(self.commit(prepared, ledger))
    }

    /// The caller must exclusively retain this outbox and ledger until commit
    /// or cancellation. All queue and ledger capacity is acquired here.
    pub(crate) fn prepare(
        &mut self,
        frames: Vec<Vec<u8>>,
        control: bool,
        ledger: &mut Ledger,
    ) -> Result<PreparedQueue, ShellClientError> {
        let bytes: usize = frames.iter().map(Vec::len).sum();
        if !self.fits(frames.len(), bytes, control) {
            return Err(ShellClientError::QueueSaturated);
        }
        // Prepare all allocations before transferring either half. Extending
        // the already-reserved FIFO cannot leave just an ACK after unwind.
        let first = ledger.peek();
        let prepared: Vec<_> = frames
            .into_iter()
            .zip(first..)
            .map(|(bytes, ticket)| Frame {
                len: bytes.len(),
                bytes: Some(bytes.into_boxed_slice()),
                control,
                ticket,
            })
            .collect();
        let records = prepared.len();
        self.frames
            .try_reserve(records + self.reserved.as_ref().map_or(0, |r| r.slots))
            .map_err(|_| ShellClientError::QueueSaturated)?;
        ledger.reserve(records)?;
        Ok(PreparedQueue {
            frames: prepared,
            bytes,
            control,
        })
    }

    pub(crate) fn commit(&mut self, prepared: PreparedQueue, ledger: &mut Ledger) -> Admission {
        let records = prepared.frames.len();
        self.frames.extend(prepared.frames);
        self.bytes += prepared.bytes;
        if !prepared.control {
            self.bulk_records += records;
            self.bulk_bytes += prepared.bytes;
        }
        ledger.issue(records)
    }
    /// Whether `slots` more frames of `bytes` would fit right now.
    pub(crate) fn has_room(&self, slots: usize, bytes: usize, control: bool) -> bool {
        self.fits(slots, bytes, control)
    }

    pub(crate) fn front(&self) -> Option<&[u8]> {
        self.frames.front().and_then(|frame| frame.bytes.as_deref())
    }

    /// Transfer bytes without releasing the unit's admission or custody ticket.
    pub(crate) fn take_front_bytes(&mut self) -> Box<[u8]> {
        self.take_bytes(0)
    }
    pub(crate) fn take_bytes(&mut self, index: usize) -> Box<[u8]> {
        self.frames
            .get_mut(index)
            .and_then(|frame| frame.bytes.take())
            .expect("the current file operation takes its payload once")
    }
    pub(crate) fn ticket_at(&self, index: usize) -> u64 {
        self.frames[index].ticket
    }
    /// The front unit's ticket.
    pub(crate) fn front_ticket(&self) -> Option<u64> {
        self.frames.front().map(|frame| frame.ticket)
    }

    /// Releases the front unit after the file wire settles its custody.
    pub(crate) fn retire_front(&mut self) {
        let frame = self
            .frames
            .pop_front()
            .expect("retiring requires an owned frame");
        self.bytes -= frame.len;
        if !frame.control {
            self.bulk_records -= 1;
            self.bulk_bytes -= frame.len;
        }
    }

    /// Every unit still queued, front first, by ticket.
    pub(crate) fn tickets(&self) -> impl Iterator<Item = u64> + '_ {
        self.frames.iter().map(|frame| frame.ticket)
    }
}

#[cfg(test)]
#[path = "../tests/support/outbox.rs"]
mod tests;
