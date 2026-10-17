//! Delayed-content capacity reservations. The token is identity only; the
//! connection owns credits and validates each use against its live reservation.
use crate::wire::Outbound;
use crate::{Admission, QueueReservation, ShellClientError, ShellConnection};
use sophia_shell_protocol::{ShellContentRecord, TransactionId};

impl ShellConnection {
    pub fn queue_limits(&self) -> crate::QueueLimits {
        self.output.limits()
    }

    /// Configure a live, empty admission queue, with no delayed reservation.
    /// In-flight records remain charged until settlement. Bounds are 1..=64
    /// records and 1..=2 MiB, with headroom no larger than either total. This
    /// changes logical limits, not retained allocation capacity. Configure
    /// before giving the connection to an owning role facade.
    pub fn set_queue_limits(&mut self, limits: crate::QueueLimits) -> Result<(), ShellClientError> {
        self.wire.io_interest()?;
        self.output.set_limits(limits)
    }
    /// Reserve up to `slots` encoded units and `bytes` before choosing records.
    /// One reservation may be live; dispatch and admissions using the remaining
    /// capacity can continue. Credits come from the bulk allowance, preserving
    /// the control reserve. Sizes must be nonzero and within that allowance.
    /// No ticket is issued. Explicitly cancel an unused token; dropping it alone
    /// does not cancel. Unlike a prepared ContentReservation, commit still
    /// validates/encodes records and can fail allocation or value checks.
    pub fn reserve_capacity(
        &mut self,
        slots: usize,
        bytes: usize,
    ) -> Result<QueueReservation, ShellClientError> {
        self.wire.io_interest()?;
        self.output.reserve_capacity(slots, bytes)
    }

    /// Release a live reservation. Wrong-connection, cancelled or committed
    /// tokens are invalid and change nothing.
    pub fn cancel_reservation(&mut self, token: &QueueReservation) -> Result<(), ShellClientError> {
        self.output.cancel_reservation(token)
    }

    /// Admit a content operation against held credits, with no socket I/O.
    /// Invalid records, identity or sizing leave the reservation live and issue
    /// no tickets. A successful commit consumes the reservation; unused credits
    /// are released. An observed terminal connection releases it and admits
    /// nothing. Only later Submitted/Stored observations establish custody.
    pub fn commit_reserved_content(
        &mut self,
        token: &QueueReservation,
        transaction: TransactionId,
        record: &ShellContentRecord,
    ) -> Result<Admission, ShellClientError> {
        let outbound = Outbound::Content(transaction, record.clone());
        let control = outbound.is_control();
        self.commit_reserved_outbound(token, outbound, control)
    }

    /// Commit a whole candidate against held credits. It encodes as one unit;
    /// this is not an arbitrary batch of unrelated content records and does
    /// not register ContentLifecycle metadata.
    pub fn commit_reserved_content_group(
        &mut self,
        token: &QueueReservation,
        transaction: TransactionId,
        records: &[ShellContentRecord],
    ) -> Result<Admission, ShellClientError> {
        self.commit_reserved_outbound(
            token,
            Outbound::ContentGroup(transaction, records.to_vec()),
            false,
        )
    }

    fn commit_reserved_outbound(
        &mut self,
        token: &QueueReservation,
        outbound: Outbound,
        control: bool,
    ) -> Result<Admission, ShellClientError> {
        if let Err(error) = self.wire.io_interest() {
            self.output.release_reservation();
            return Err(error);
        }
        self.output.validate_reservation(token)?;
        let units = self.wire.encode(outbound)?;
        self.wire.reserve_encoded()?;
        let prepared = self
            .output
            .prepare_reserved(token, units, control, &mut self.ledger)?;
        let admission = self.output.commit(prepared, &mut self.ledger);
        self.wire.commit_encoded();
        Ok(admission)
    }
}
