//! Scripted wire load through the production worker, adapter and journal.
//! Reading events does not acknowledge them. No journal state is injected.
#![cfg(test)]
use super::runtime_adapter::tests::{configured_at, enqueue};
use super::startup::tests::Peer;
use super::*;
use crate::live_session::policy_transport_worker::{PolicyTransportCommand, PolicyTransportWorker};
use sophia_protocol::*;

pub(in crate::live_session) struct ReceiptPeer {
    peer: Peer,
    caps: u64,
}

pub(in crate::live_session) fn configured(epoch: u64) -> (PolicyTransportWorker, ReceiptPeer) {
    let (worker, peer, caps) = configured_at(epoch);
    (worker, ReceiptPeer { peer, caps })
}

impl ReceiptPeer {
    pub(in crate::live_session) fn acknowledge_receipt(
        &mut self,
        receipt: PolicyPresentationReceipt,
    ) {
        let bytes = self.peer.next_event();
        assert_eq!(
            decode_wm_file_presentation_receipt(&bytes, self.caps)
                .unwrap()
                .receipt,
            receipt
        );
        self.peer.ack(&bytes);
    }

    pub(in crate::live_session) fn exhaust_record_credit(
        &mut self,
        worker: &PolicyTransportWorker,
        receipt: PolicyPresentationReceipt,
    ) -> Instant {
        let command = |id| PolicyTransportCommand::PresentationReceipt {
            transaction: TransactionId::from_raw(id),
            receipt,
        };
        let mut total_bytes = 0;
        let mut previous_sequence = None;
        for index in 0..WM_FILE_MAX_JOURNAL_RECORDS {
            let transaction = 1000 + index as u64;
            enqueue(worker, command(transaction));
            let bytes = self.peer.next_event();
            let record = decode_wm_file_record(&bytes, WmFileClass::Event).unwrap();
            if let Some(previous) = previous_sequence {
                assert_eq!(record.header.sequence, previous + 1);
            }
            previous_sequence = Some(record.header.sequence);
            assert_eq!(
                decode_wm_file_presentation_receipt(&bytes, self.caps).unwrap(),
                WmFilePresentationReceipt {
                    transaction: TransactionId::from_raw(transaction),
                    receipt,
                }
            );
            total_bytes += bytes.len();
            // Deliberately no ACK: even records already read retain credit.
        }
        assert!(
            total_bytes < WM_FILE_MAX_BYTES,
            "record credit, not byte credit"
        );
        let blocked_since = Instant::now();
        enqueue(worker, command(2000));
        // Acceptance of the second command proves the worker took the first.
        // The first cannot enter the journal until an ACK returns record credit.
        enqueue(worker, command(2001));
        assert!(
            worker.try_command(command(2002)).is_err(),
            "owner command slot is full"
        );
        // A separate wire operation remains serviceable while the send waits.
        // It neither reads nor acknowledges journal records.
        self.peer.open(6, b"api", 0);
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            worker.try_command(command(2002)).is_err(),
            "owner command slot remains full without ACK credit"
        );
        assert!(
            matches!(worker.try_event(), Ok(None)),
            "pressure must precede transport failure"
        );
        blocked_since
    }
}
