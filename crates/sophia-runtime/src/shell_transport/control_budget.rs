use sophia_protocol::shell_files::SHELL_FILE_HEADER_BYTES;

use super::outbound::{Admitted, OutboundRecord};
use super::wire::Wire;
use super::{ShellComponentTransport, ShellTransportError};

/// The byte charge of one control credit. Every fixed r5 lifecycle response
/// fits it as a whole record on each admitted wire (the largest, a file
/// `AllocationResult`, is 32 + 168 bytes); `admit_record` refuses one that
/// does not. Variable output and indicator snapshots are bulk and cannot
/// spend it.
pub(super) const CONTROL_RECORD_BYTES: usize = 256;
/// The native launcher's credit also covers its `NativeInput` (32 + 398).
const NATIVE_CONTROL_RECORD_BYTES: usize = 512;
/// The FIFO bound of a connection admitted without content limits.
const UNLIMITED_RECORDS: usize = 64;
const UNLIMITED_BYTES: usize = 2 * 1024 * 1024;

/// How an owner classes one record before transfer.
pub(super) enum Class {
    /// Output facts and snapshots: charged by their body bytes.
    Bulk,
    /// A response owning one control credit, which must fit `limit` as a
    /// whole wire record; `oversize` names the owner's refusal otherwise.
    Control {
        limit: usize,
        oversize: ShellTransportError,
    },
}

impl ShellComponentTransport {
    pub(super) fn control_record_bytes(&self) -> usize {
        if self.file_descriptor() {
            super::files::DESCRIPTOR_RECORD_BYTES
        } else if self.supports_native_launcher() {
            NATIVE_CONTROL_RECORD_BYTES
        } else {
            CONTROL_RECORD_BYTES
        }
    }

    /// Records in the output order: the typed FIFO and, on the socket, the
    /// frames only it carries.
    pub(super) fn fifo_records(&self) -> usize {
        self.output.records() + self.socket().map_or(0, |socket| socket.lane_records())
    }

    pub(super) fn fifo_controls(&self) -> usize {
        self.output.controls() + self.socket().map_or(0, |socket| socket.lane_controls())
    }

    pub(super) fn fifo_bulk_bytes(&self) -> usize {
        self.output.bulk_bytes() + self.socket().map_or(0, |socket| socket.lane_bulk_bytes())
    }

    /// Every retained output charge, control and bulk.
    pub(super) fn fifo_bytes(&self) -> usize {
        self.output.charged() + self.socket().map_or(0, |socket| socket.lane_bytes())
    }

    pub(super) fn fifo_is_empty(&self) -> bool {
        self.fifo_records() == 0
    }

    /// Checks one record's encoding and its wire's per-record bounds, before
    /// any owner changes state. The returned record can only be transferred.
    pub(super) fn admit_record(
        &self,
        record: OutboundRecord,
        class: Class,
    ) -> Result<Admitted, ShellTransportError> {
        let (_, body) = record.native()?;
        let (control, limit) = match class {
            Class::Bulk => (false, None),
            Class::Control { limit, oversize } => (true, Some((limit, oversize))),
        };
        match self.wire.as_ref() {
            Some(Wire::Socket(socket)) => socket.admits(&record, limit)?,
            Some(Wire::Files(_)) | None => {
                if let Some((limit, oversize)) = limit
                    && SHELL_FILE_HEADER_BYTES + body.len() > limit
                {
                    return Err(oversize);
                }
            }
        }
        Ok(Admitted {
            record,
            control,
            charge: body.len(),
        })
    }

    /// The custody transfer into the FIFO. Admission and capacity were
    /// checked first; no I/O or refusal follows, so the producer may release
    /// its exact obligation afterwards without ambiguity.
    pub(super) fn transfer_record(&mut self, admitted: Admitted) {
        self.output.push(admitted);
    }

    /// Existing reducer credits name exact accepted obligations. Queued events
    /// remain charged there until custody moves into the same FIFO.
    pub(super) fn control_capacity_available(
        &self,
        epochs: &crate::ContentEpochRegistry,
        additional: usize,
    ) -> bool {
        let Some(limits) = &self.content_limits else {
            return false;
        };
        let (bulk_records, bulk_bytes) = epochs.bulk_occupancy(self.store_grant);
        let reserved = self.reserved_credits(epochs);
        let controls = reserved - bulk_records + self.fifo_controls() + additional;
        let records = reserved + self.fifo_records() + additional;
        records <= limits.max_control_records as usize
            && controls.saturating_mul(self.control_record_bytes())
                <= limits.reserved_control_queue_bytes as usize
            && bulk_bytes
                .saturating_add(self.fifo_bulk_bytes())
                .saturating_add(controls.saturating_mul(self.control_record_bytes()))
                <= limits.max_output_queue_bytes as usize
    }

    fn reserved_credits(&self, epochs: &crate::ContentEpochRegistry) -> usize {
        epochs.control_occupancy(self.store_grant)
            + self.action_cancellations.len()
            + usize::from(self.indicator_response.is_some())
            + usize::from(self.catalog_response.is_some())
            + self.native_control.credits()
            + self.descriptor_state.response_credits
            + self.tab_state.response_credits
            + self.reference_state.response_credits
            + self.launcher_state.response_credits
    }

    /// Checks the post-transfer inventory of one admitted record without
    /// releasing the producer's credit. `transfer` names a record whose credit
    /// (or, for bulk, registry charge of the same size) moves with it.
    pub(super) fn record_capacity_available(
        &self,
        epochs: &crate::ContentEpochRegistry,
        charge: usize,
        control: bool,
        transfer: bool,
    ) -> bool {
        let Some(limits) = &self.content_limits else {
            return false;
        };
        let (bulk_records, bulk_bytes) = epochs.bulk_occupancy(self.store_grant);
        let reserved = self.reserved_credits(epochs);
        let Some(records) = reserved.checked_sub(usize::from(transfer)) else {
            return false;
        };
        let Some(controls) =
            (reserved - bulk_records).checked_sub(usize::from(transfer && control))
        else {
            return false;
        };
        let Some(bulk_bytes) =
            bulk_bytes.checked_sub(if transfer && !control { charge } else { 0 })
        else {
            return false;
        };
        let controls = controls + self.fifo_controls() + usize::from(control);
        let records = records + self.fifo_records() + 1;
        let bulk = bulk_bytes + self.fifo_bulk_bytes() + if control { 0 } else { charge };
        records <= limits.max_control_records as usize
            && controls.saturating_mul(self.control_record_bytes())
                <= limits.reserved_control_queue_bytes as usize
            && bulk.saturating_add(controls.saturating_mul(self.control_record_bytes()))
                <= limits.max_output_queue_bytes as usize
            && (control
                || bulk
                    <= limits
                        .max_output_queue_bytes
                        .saturating_sub(limits.reserved_control_queue_bytes)
                        as usize)
    }

    pub(super) fn bulk_capacity_available(
        &self,
        epochs: &crate::ContentEpochRegistry,
        bytes: usize,
    ) -> bool {
        if self.content_limits.is_some() {
            self.record_capacity_available(epochs, bytes, false, false)
        } else {
            self.fifo_records()
                + usize::from(self.indicator_response.is_some())
                + self.descriptor_state.response_credits
                + self.tab_state.response_credits
                + self.reference_state.response_credits
                + self.launcher_state.response_credits
                < UNLIMITED_RECORDS
                && self.fifo_bytes().saturating_add(bytes).saturating_add(
                    (usize::from(self.indicator_response.is_some())
                        + self.descriptor_state.response_credits)
                        .saturating_add(self.tab_state.response_credits)
                        .saturating_add(self.reference_state.response_credits)
                        .saturating_add(self.launcher_state.response_credits)
                        * self.control_record_bytes(),
                ) <= UNLIMITED_BYTES
        }
    }

    /// The FIFO bound of a connection without content limits, for one more
    /// control record of `bytes`.
    pub(super) fn unlimited_capacity_available(&self, bytes: usize) -> bool {
        self.fifo_records()
            + self.descriptor_state.response_credits
            + self.tab_state.response_credits
            + self.reference_state.response_credits
            + self.launcher_state.response_credits
            < UNLIMITED_RECORDS
            && self.fifo_bytes().saturating_add(bytes).saturating_add(
                (self.descriptor_state.response_credits
                    + self.tab_state.response_credits
                    + self.reference_state.response_credits
                    + self.launcher_state.response_credits)
                    * self.control_record_bytes(),
            ) <= UNLIMITED_BYTES
    }
}

#[cfg(test)]
#[path = "../../tests/support/shell_control_budget.rs"]
mod tests;
