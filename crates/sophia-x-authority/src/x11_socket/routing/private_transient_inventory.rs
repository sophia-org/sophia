/// Request effects without a common hold. Storage is reserved before producer
/// exposure, and pending source custody stays here across an interrupted effect.
#[cfg(unix)]
struct PrivateTransientInventory {
    pending: Option<private_native::Transient>,
    records: Vec<PrivateTransientRecord>,
    cursor: usize,
}

#[cfg(unix)]
struct PrivateTransientRecord {
    source: private_native::Transient,
    custody: PrivateDeliveryCustody,
}

#[cfg(unix)]
impl PrivateTransientInventory {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            pending: None,
            records: Vec::with_capacity(capacity),
            cursor: 0,
        }
    }

    fn outstanding(&self) -> usize {
        self.records.len() + usize::from(self.pending.is_some())
    }

    fn owes_visit(&self) -> bool {
        self.records.iter().any(|record| {
            record.custody.owes_handover()
                || (record.custody.dispatch == PrivateDispatchPhase::Enqueued
                    && record.custody.outcome_seen.is_none())
        })
    }

    /// One observation, after the caller charges and starts its watchdog.
    /// A failed/unknown write remains owned. Only the exact cell's established
    /// flush or recipient termination permits dropping this non-ledger effect.
    fn observe_one(&mut self) -> Option<bool> {
        if self.records.is_empty() {
            return None;
        }
        self.cursor %= self.records.len();
        let index = self.cursor;
        self.cursor += 1;
        let custody = &mut self.records[index].custody;
        if custody.dispatch != PrivateDispatchPhase::Enqueued {
            return Some(false);
        }
        let Some(answer) = custody.completion.as_ref().and_then(|cell| cell.answer()) else {
            return Some(false);
        };
        custody.outcome_seen = Some(answer.outcome);
        if matches!(
            answer.outcome,
            XAuthorityInputDeliveryOutcome::Flushed
                | XAuthorityInputDeliveryOutcome::ClientDisconnected
        ) {
            self.records.remove(index);
            Some(true)
        } else {
            Some(false)
        }
    }
}
