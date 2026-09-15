use super::ShellSessionTransport;

// All fixed r5 lifecycle responses fit this frame envelope, including the IPC
// header. Variable output/indicator snapshots are bulk and cannot spend it.
pub(super) const CONTROL_FRAME_BYTES: usize = 256;

impl ShellSessionTransport {
    /// Existing reducer credits name exact accepted obligations. Queued events
    /// remain charged there until custody moves into the same wire FIFO.
    pub(super) fn control_capacity_available(&self, additional: usize) -> bool {
        let Some(limits) = &self.content_limits else {
            return false;
        };
        let (bulk_records, bulk_bytes) = self.content_epochs.active_bulk_occupancy();
        let reserved =
            self.content_epochs.active_control_occupancy() + self.action_cancellations.len();
        let controls = reserved - bulk_records + self.output.controls() + additional;
        let records = reserved + self.output.records() + additional;
        records <= limits.max_control_records as usize
            && controls.saturating_mul(CONTROL_FRAME_BYTES)
                <= limits.reserved_control_queue_bytes as usize
            && bulk_bytes
                .saturating_add(self.output.bulk_bytes())
                .saturating_add(controls.saturating_mul(CONTROL_FRAME_BYTES))
                <= limits.max_output_queue_bytes as usize
    }

    /// Check the post-transfer inventory without releasing the producer's
    /// credit. Encoding and admission may fail; neither changes either owner.
    pub(super) fn frame_capacity_available(
        &self,
        bytes: usize,
        control: bool,
        transfer: bool,
    ) -> bool {
        let Some(limits) = &self.content_limits else {
            return false;
        };
        let (bulk_records, bulk_bytes) = self.content_epochs.active_bulk_occupancy();
        let reserved =
            self.content_epochs.active_control_occupancy() + self.action_cancellations.len();
        let Some(records) = reserved.checked_sub(usize::from(transfer)) else {
            return false;
        };
        let Some(controls) =
            (reserved - bulk_records).checked_sub(usize::from(transfer && control))
        else {
            return false;
        };
        let Some(bulk_bytes) = bulk_bytes.checked_sub(if transfer && !control { bytes } else { 0 })
        else {
            return false;
        };
        let controls = controls + self.output.controls() + usize::from(control);
        let records = records + self.output.records() + 1;
        let bulk = bulk_bytes + self.output.bulk_bytes() + if control { 0 } else { bytes };
        records <= limits.max_control_records as usize
            && controls.saturating_mul(CONTROL_FRAME_BYTES)
                <= limits.reserved_control_queue_bytes as usize
            && bulk.saturating_add(controls.saturating_mul(CONTROL_FRAME_BYTES))
                <= limits.max_output_queue_bytes as usize
            && (control
                || bulk
                    <= limits
                        .max_output_queue_bytes
                        .saturating_sub(limits.reserved_control_queue_bytes)
                        as usize)
    }

    pub(super) fn bulk_capacity_available(&self, bytes: usize) -> bool {
        if self.content_limits.is_some() {
            self.frame_capacity_available(bytes, false, false)
        } else {
            self.output.records() < 64 && self.output.len().saturating_add(bytes) <= 2 * 1024 * 1024
        }
    }
}

#[cfg(test)]
#[path = "../../tests/support/shell_control_budget.rs"]
mod tests;
