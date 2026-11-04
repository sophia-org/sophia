//! SFT responses transfer their existing store credit into the shared FIFO.
use super::control_budget::{CONTROL_RECORD_BYTES, Class};
use super::outbound::{Admitted, OutboundRecord};
use super::{ShellComponentTransport, ShellTransportError};
use crate::frame_credits::FrameStreamKey;
use crate::{
    ContentCandidateEvent, ContentEpochRegistry, ContentFrameStreamEvent, ContentFrameStreamRecord,
};
use sophia_protocol::ShellContentRecord;
use sophia_protocol::shell_files::{SHELL_FRAME_CAPABILITY, ShellFrameRecord, ShellFrameStreamKey};

fn wire_key(key: FrameStreamKey) -> ShellFrameStreamKey {
    ShellFrameStreamKey {
        grant: key.grant,
        output: key.output,
        allocation: key.allocation,
        facts_generation: key.facts_generation,
        stream_id: key.stream_id,
    }
}

fn stream_record(event: ContentFrameStreamEvent) -> Result<ShellFrameRecord, ShellTransportError> {
    let transaction = event.transaction;
    Ok(match event.record {
        ContentFrameStreamRecord::Opened {
            key,
            credits: 1,
            ttl_ms,
        } => ShellFrameRecord::Opened {
            transaction,
            key: wire_key(key),
            ttl_millis: ttl_ms,
        },
        ContentFrameStreamRecord::Opened { .. } => {
            return Err(ShellTransportError::WrongContentRecord);
        }
        ContentFrameStreamRecord::Closed(notice) => ShellFrameRecord::Closed {
            transaction,
            key: wire_key(notice.key),
            reason: notice.reason as u16,
        },
        ContentFrameStreamRecord::Refused {
            request_kind,
            grant,
            output,
            stream_id,
            candidate_generation,
            reason,
        } => ShellFrameRecord::Refused {
            transaction,
            grant,
            output,
            stream_id,
            candidate_generation,
            request_kind,
            reason: reason as u16,
        },
    })
}

fn outcome_record(event: &ContentCandidateEvent) -> Result<ShellFrameRecord, ShellTransportError> {
    let credit = event
        .frame_credit
        .ok_or(ShellTransportError::WrongContentRecord)?;
    let ShellContentRecord::CandidateOutcome(outcome) = &event.record else {
        return Err(ShellTransportError::WrongContentRecord);
    };
    if outcome.grant != credit.key.grant || outcome.output != credit.key.output {
        return Err(ShellTransportError::WrongContentGrant);
    }
    Ok(ShellFrameRecord::Outcome {
        transaction: event.transaction,
        outcome: outcome.clone(),
        stream_id: credit.key.stream_id,
        allocation: credit.key.allocation,
        facts_generation: credit.key.facts_generation,
        credit_expires_msec: credit.expires_msec.unwrap_or(0),
    })
}

impl ShellComponentTransport {
    fn prepare_frame_record(
        &self,
        epochs: &ContentEpochRegistry,
        record: ShellFrameRecord,
    ) -> Result<Admitted, ShellTransportError> {
        let grant = match &record {
            ShellFrameRecord::Opened { key, .. } | ShellFrameRecord::Closed { key, .. } => {
                key.grant
            }
            ShellFrameRecord::Refused { grant, .. } => *grant,
            ShellFrameRecord::Outcome { outcome, .. } => outcome.grant,
            _ => return Err(ShellTransportError::WrongContentRecord),
        };
        if self.capabilities & SHELL_FRAME_CAPABILITY == 0 {
            return Err(ShellTransportError::MissingCapability);
        }
        if self.content_grant != Some(grant) || self.store_grant != grant {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let admitted = self.admit_record(
            OutboundRecord::Frame(record),
            Class::Control {
                limit: CONTROL_RECORD_BYTES,
                oversize: ShellTransportError::ContentQueueSaturated,
            },
        )?;
        if !self.record_capacity_available(epochs, admitted.charge, true, true) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        Ok(admitted)
    }

    /// Publish ordered SFT responses using current owner time for terminal credit.
    /// Backpressure keeps the exact store event and all resource obligations owned.
    ///
    /// # Errors
    /// Invalid identity, encoding, clock or capacity leaves the refused head owned
    /// by the store. Earlier whole responses may already have transferred.
    pub fn flush_content_frame_events(
        &mut self,
        epochs: &mut ContentEpochRegistry,
        now_msec: u64,
    ) -> Result<(), ShellTransportError> {
        loop {
            let store = epochs
                .active_candidates(self.store_grant)
                .ok_or(ShellTransportError::MissingCapability)?;
            if let Some(event) = store.pending_frame_stream_event().copied() {
                let admitted = self.prepare_frame_record(epochs, stream_record(event)?)?;
                self.transfer_record(admitted);
                let taken = epochs
                    .active_candidates_mut(self.store_grant)
                    .expect("grant retained through response transfer")
                    .take_frame_stream_event();
                assert_eq!(taken, Some(event), "published the exact stream FIFO head");
                continue;
            }
            let Some(event) = store.pending_event().cloned() else {
                return Ok(());
            };
            if event.frame_credit.is_none() {
                self.flush_content_candidate_events(epochs)?;
                continue;
            }
            let mut admitted = self.prepare_frame_record(epochs, outcome_record(&event)?)?;
            let (resources, store) = epochs
                .active_pair_mut(self.store_grant)
                .expect("grant retained through response transfer");
            store.transfer_frame_event_at(resources, now_msec, |published| {
                let OutboundRecord::Frame(ShellFrameRecord::Outcome {
                    credit_expires_msec,
                    ..
                }) = &mut admitted.record
                else {
                    unreachable!("admitted one complete SFT outcome");
                };
                // Only this fixed-width field changes after encoding/capacity admission.
                *credit_expires_msec = published
                    .frame_credit
                    .expect("SFT outcome identity")
                    .expires_msec
                    .unwrap_or(0);
                self.transfer_record(admitted);
                Ok(())
            })?;
        }
    }
}
