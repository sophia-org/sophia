use sophia_9p::export::*;
use sophia_9p::{ConnectionId, Errno, OpenFlags};
use sophia_portal::files::*;
use sophia_protocol::portal_files::*;
use sophia_protocol::*;
use std::time::{Duration, Instant};

type Node = PortalFileNode;

#[path = "support/portal_file_wire.rs"]
mod wire;

#[path = "support/portal_file_capacity.rs"]
mod capacity;

struct Authority {
    revision: u64,
    rights: bool,
    allow: bool,
    live: bool,
    now: u64,
    generation: u64,
}
impl PortalFileAuthority for Authority {
    fn revision(&self) -> u64 {
        self.revision
    }
    fn source_generation(&self, _request: &PortalRequest) -> Option<u64> {
        Some(self.generation)
    }
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        if ![1, 2].contains(&id.raw()) {
            return None;
        }
        NamespaceContext::new(
            id,
            NamespaceProfile::Confined,
            if self.rights {
                NamespaceCapabilities::ALL
            } else {
                NamespaceCapabilities::NONE
            },
        )
    }
    fn now_msec(&self) -> u64 {
        self.now
    }
    fn live(&self) -> bool {
        self.live
    }
    fn allow(&self, _: &PortalRequest) -> bool {
        self.allow
    }
}
fn admission(epoch: u64) -> PortalRequesterAdmission {
    PortalRequesterAdmission {
        connection: ConnectionId(epoch),
        custody: PortalRequesterCustody::ProtectedPeer(PeerCredentials {
            pid: 42,
            uid: 1000,
            gid: 1000,
        }),
        epoch,
        source: NamespaceId::from_raw(1),
        target: NamespaceId::from_raw(2),
    }
}
fn attach(export: &mut PortalFileExport<Authority>, epoch: u64) -> Result<Attachment<Node>, Errno> {
    export.attach(&AttachContext {
        connection: ConnectionId(epoch),
        peer: Some(PeerCredentials {
            pid: 42,
            uid: 1000,
            gid: 1000,
        }),
        uname: b"not authority",
        aname: b"not a namespace",
        n_uname: u32::MAX,
    })
}
struct Fixture {
    export: PortalFileExport<Authority>,
    next: u64,
    offset: u64,
    epoch: u64,
}
impl Fixture {
    fn new(limits: PortalFileLimits) -> Self {
        let mut export = PortalFileExport::new(
            Authority {
                revision: 1,
                rights: true,
                allow: true,
                live: true,
                now: 100,
                generation: 9,
            },
            limits,
        )
        .unwrap();
        export.admit(admission(1)).unwrap();
        attach(&mut export, 1).unwrap();
        Self {
            export,
            next: 1,
            offset: 0,
            epoch: 1,
        }
    }
    fn send(&mut self, candidate: PortalFileCandidate) -> Result<(), Errno> {
        let bytes = encode_portal_file_candidate(self.epoch, self.next, &candidate).unwrap();
        let mut tx = self.export.open(&Node::Transaction, OpenFlags(2))?;
        self.export.write(&Node::Transaction, &mut tx, 0, &bytes)?;
        let control = encode_portal_file_submit(PortalFileSubmit {
            connection_epoch: self.epoch,
            submission_id: self.next,
            candidate_bytes: bytes.len() as u32,
        })
        .unwrap();
        let mut submit = self.export.open(&Node::Submit, OpenFlags(1))?;
        let result = self
            .export
            .write(&Node::Submit, &mut submit, 0, &control)
            .map(|_| ());
        self.export.release(Node::Transaction, Some(tx));
        self.export.release(Node::Submit, Some(submit));
        if result.is_ok() {
            self.next += 1;
        }
        result
    }
    fn events(&mut self, ack: bool) -> Vec<(u64, PortalFileEvent)> {
        let mut h = self.export.open(&Node::Events, OpenFlags(0)).unwrap();
        let bytes = match self
            .export
            .read(&Node::Events, &mut h, self.offset, 32768)
            .unwrap()
        {
            ReadOutcome::Ready(b) => b,
            ReadOutcome::Pending => Vec::new(),
        };
        self.offset += bytes.len() as u64;
        let mut tail = bytes.as_slice();
        let mut events = Vec::new();
        while !tail.is_empty() {
            let len = u32::from_le_bytes(tail[..4].try_into().unwrap()) as usize;
            let (header, event) = decode_portal_file_event(&tail[..len], self.epoch).unwrap();
            events.push((header.sequence, event));
            tail = &tail[len..];
        }
        self.export.release(Node::Events, Some(h));
        if ack && let Some((seq, _)) = events.last() {
            self.ack(*seq).unwrap();
        }
        events
    }
    fn ack(&mut self, seq: u64) -> Result<(), Errno> {
        let mut h = self.export.open(&Node::Ack, OpenFlags(1))?;
        let bytes = encode_portal_file_ack(PortalFileAck {
            connection_epoch: self.epoch,
            sequence: seq,
        })
        .unwrap();
        self.export.write(&Node::Ack, &mut h, 0, &bytes).map(|_| ())
    }
    fn negotiate(&mut self) {
        self.send(PortalFileCandidate::Negotiate(PortalNegotiate {
            minimum_revision: 1,
            maximum_revision: 1,
        }))
        .unwrap();
        assert!(matches!(
            self.events(true).last().unwrap().1,
            PortalFileEvent::Negotiated
        ));
    }
    fn request(&mut self, id: u64) -> PortalBrokerResponseDecision {
        self.send(request(id)).unwrap();
        let events = self.events(true);
        match &events.last().unwrap().1 {
            PortalFileEvent::Decision(d) => d.decision.clone(),
            _ => panic!("missing decision"),
        }
    }
    fn begin(&mut self, id: u64, bytes: &[u8]) -> PortalFileHandle {
        self.send(PortalFileCandidate::PayloadBegin(PortalPayloadBegin {
            transfer: PortalTransferId::from_raw(id),
            total_bytes: bytes.len() as u64,
        }))
        .unwrap();
        self.events(true);
        let mut h = self.export.open(&Node::Slot, OpenFlags(1)).unwrap();
        self.export.write(&Node::Slot, &mut h, 0, bytes).unwrap();
        h
    }
    fn end(&mut self, id: u64, len: u64) -> Result<(), Errno> {
        self.send(PortalFileCandidate::PayloadEnd(PortalPayloadEnd {
            transfer: PortalTransferId::from_raw(id),
            total_bytes: len,
        }))
    }
}
fn request(id: u64) -> PortalFileCandidate {
    PortalFileCandidate::TransferRequest(PortalRequest {
        transfer: PortalTransfer {
            transfer: PortalTransferId::from_raw(id),
            source_namespace: NamespaceId::from_raw(1),
            target_namespace: NamespaceId::from_raw(2),
            kind: PortalTransferKind::Clipboard,
            mime_type: Some("UTF8_STRING".into()),
            byte_size: 0,
            decision: PortalDecision::Pending,
            generation: 9,
        },
        deadline_msec: 10000,
    })
}

#[test]
fn admission_is_exact_and_request_permissions_come_from_owner() {
    let mut f = Fixture::new(PortalFileLimits::defaults(1));
    assert!(matches!(attach(&mut f.export, 1), Err(Errno::EACCES)));
    assert_eq!(
        f.export.check(&Access {
            connection: ConnectionId(2),
            epoch: Epoch(1),
            node: &Node::Root,
            operation: Operation::Read
        }),
        Err(Errno::ESTALE)
    );
    f.negotiate();
    f.export.authority_mut().rights = false;
    assert_eq!(f.request(1), PortalBrokerResponseDecision::Denied);
    f.export.authority_mut().rights = true;
    f.export.authority_mut().allow = false;
    assert_eq!(f.request(2), PortalBrokerResponseDecision::Denied);
    let PortalFileCandidate::TransferRequest(mut foreign) = request(3) else {
        unreachable!()
    };
    foreign.transfer.target_namespace = NamespaceId::from_raw(3);
    assert_eq!(
        f.send(PortalFileCandidate::TransferRequest(foreign)),
        Err(Errno::EACCES)
    );
    assert!(
        f.events(false).is_empty(),
        "foreign request acquired no custody"
    );
}

#[test]
fn end_hands_off_once_and_completed_requires_recipient_success() {
    let mut f = Fixture::new(PortalFileLimits::defaults(1));
    f.negotiate();
    assert!(matches!(
        f.request(1),
        PortalBrokerResponseDecision::Allowed(_)
    ));
    let mut writer = f.begin(1, b"paste");
    assert!(matches!(
        f.export.open(&Node::Slot, OpenFlags(1)),
        Err(Errno(16))
    ));
    assert_eq!(f.export.write(&Node::Slot, &mut writer, 0, b"paste"), Ok(5));
    assert_eq!(
        f.export.write(&Node::Slot, &mut writer, 0, b"other"),
        Err(Errno::EINVAL)
    );
    assert_eq!(f.end(1, 4), Err(Errno::EINVAL));
    assert!(f.export.take_execution().is_none());
    f.end(1, 5).unwrap();
    assert!(
        f.events(true)
            .iter()
            .all(|(_, e)| matches!(e, PortalFileEvent::Submitted(_)))
    );
    assert_eq!(
        f.export.write(&Node::Slot, &mut writer, 0, b"paste"),
        Err(Errno::ESTALE)
    );
    let work = f.export.take_execution().unwrap();
    assert_eq!(work.payload, b"paste");
    assert_eq!(f.end(1, 5), Err(Errno(114)));
    f.export
        .settle_execution(work.grant.transfer, true)
        .unwrap();
    assert!(matches!(
        f.events(true).last().unwrap().1,
        PortalFileEvent::TransferOutcome(PortalTransferOutcome {
            state: PortalGrantState::Completed,
            ..
        })
    ));
    assert_eq!(
        f.export.settle_execution(work.grant.transfer, true),
        Err(Errno(114))
    );
    f.export.release(Node::Slot, Some(writer));
    assert!(f.events(false).is_empty());
}

#[test]
fn stale_generation_permissions_and_writer_clunk_prevent_execution() {
    for mode in 0..4 {
        let mut f = Fixture::new(PortalFileLimits::defaults(1));
        f.negotiate();
        f.request(1);
        let mut writer = f.begin(1, b"text");
        match mode {
            0 => f.export.authority_mut().generation = 10,
            1 => f.export.authority_mut().rights = false,
            2 => f.export.authority_mut().now = 10000,
            _ => {
                f.export.release(Node::Slot, Some(writer));
                assert!(f.end(1, 4).is_err());
                assert!(f.export.take_execution().is_none());
                continue;
            }
        }
        assert_eq!(
            f.export.write(&Node::Slot, &mut writer, 0, b"text"),
            Err(Errno::ESTALE)
        );
        assert!(f.end(1, 4).is_err());
        assert!(f.export.take_execution().is_none());
        let events = f.events(true);
        assert!(matches!(
            events.last().unwrap().1,
            PortalFileEvent::TransferOutcome(_)
        ));
    }
}

#[test]
fn incomplete_and_invalid_text_never_take_execution_custody() {
    let mut f = Fixture::new(PortalFileLimits::defaults(1));
    f.negotiate();
    f.request(1);
    f.send(PortalFileCandidate::PayloadBegin(PortalPayloadBegin {
        transfer: PortalTransferId::from_raw(1),
        total_bytes: 2,
    }))
    .unwrap();
    f.events(true);
    let mut writer = f.export.open(&Node::Slot, OpenFlags(1)).unwrap();
    f.export
        .write(&Node::Slot, &mut writer, 0, &[0xff])
        .unwrap();
    assert_eq!(f.end(1, 2), Err(Errno::EINVAL));
    f.export
        .write(&Node::Slot, &mut writer, 1, &[0xff])
        .unwrap();
    assert_eq!(f.end(1, 2), Err(Errno::EINVAL));
    assert!(f.export.take_execution().is_none());
    f.export.release(Node::Slot, Some(writer));
}

#[test]
fn history_and_qids_survive_reconnect_and_issued_work_blocks_replacement() {
    let mut limits = PortalFileLimits::defaults(1);
    limits.max_transfers = 1;
    limits.max_retained_transfers = 1;
    let mut f = Fixture::new(limits);
    f.negotiate();
    f.request(1);
    let writer = f.begin(1, b"x");
    f.end(1, 1).unwrap();
    f.events(true);
    let work = f.export.take_execution().unwrap();
    let old = f.export.describe(&Node::Root, None).qid_path;
    f.export.disconnect(ConnectionId(1));
    assert_eq!(f.export.admit(admission(2)), Err(Errno(16)));
    f.export
        .settle_execution(work.grant.transfer, false)
        .unwrap();
    f.export.admit(admission(2)).unwrap();
    attach(&mut f.export, 2).unwrap();
    assert!(f.export.describe(&Node::Root, None).qid_path > old);
    f.epoch = 2;
    f.next = 1;
    f.offset = 0;
    f.negotiate();
    assert_eq!(f.request(1), PortalBrokerResponseDecision::Denied);
    assert_eq!(f.send(request(2)), Err(Errno::ENOSPC));
    f.export.release(Node::Slot, Some(writer));
    assert!(f.events(false).is_empty());
}

#[test]
fn terminal_credit_survives_journal_pressure_without_spending_refused_identity() {
    let mut limits = PortalFileLimits::defaults(1);
    limits.journal_records = 4;
    limits.journal_bytes = 280;
    let mut f = Fixture::new(limits);
    f.negotiate();
    f.send(request(1)).unwrap();
    let first = f.events(false);
    f.ack(first[0].0).unwrap();
    assert_eq!(f.send(request(2)), Err(Errno::EAGAIN));
    f.export.authority_mut().rights = false;
    f.export.expire(Instant::now());
    f.export.authority_mut().rights = true;
    assert!(matches!(
        f.events(true).last().unwrap().1,
        PortalFileEvent::TransferOutcome(_)
    ));
    assert!(
        matches!(f.request(2), PortalBrokerResponseDecision::Allowed(_)),
        "backpressure must not burn transfer identity"
    );
}

#[test]
fn refusal_has_fixed_drain_deadline_and_ack_progress_is_not_authorization() {
    let mut f = Fixture::new(PortalFileLimits::defaults(1));
    f.send(PortalFileCandidate::Negotiate(PortalNegotiate {
        minimum_revision: 2,
        maximum_revision: 2,
    }))
    .unwrap();
    let events = f.events(false);
    assert!(matches!(events[1].1, PortalFileEvent::Refused));
    f.ack(events[0].0).unwrap();
    assert_eq!(f.send(request(1)), Err(Errno::EACCES));
    f.export.expire(Instant::now() + Duration::from_secs(3));
    assert!(matches!(
        f.export.open(&Node::Events, OpenFlags(0)),
        Err(Errno::ESTALE)
    ));
}

#[test]
fn queued_payload_is_reclaimed_on_disconnect_and_can_never_execute_later() {
    let mut f = Fixture::new(PortalFileLimits::defaults(1));
    f.negotiate();
    f.request(1);
    let writer = f.begin(1, b"x");
    f.end(1, 1).unwrap();
    f.events(true);
    f.export.disconnect(ConnectionId(1));
    assert!(f.export.take_execution().is_none());
    f.export.admit(admission(2)).unwrap();
    attach(&mut f.export, 2).unwrap();
    f.epoch = 2;
    f.next = 1;
    f.offset = 0;
    f.negotiate();
    assert_eq!(
        f.send(PortalFileCandidate::TransferCancel(
            PortalTransferId::from_raw(1)
        )),
        Err(Errno::EINVAL)
    );
    f.export.release(Node::Slot, Some(writer));
    assert!(matches!(
        f.request(2),
        PortalBrokerResponseDecision::Allowed(_)
    ));
}

#[test]
fn source_generation_is_owner_derived_and_rechecked_at_executor_handoff() {
    let mut f = Fixture::new(PortalFileLimits::defaults(1));
    f.negotiate();
    f.export.authority_mut().generation = 10;
    assert_eq!(f.request(1), PortalBrokerResponseDecision::Denied);
    f.export.authority_mut().generation = 9;
    assert!(
        matches!(f.request(1), PortalBrokerResponseDecision::Allowed(_)),
        "stale source denial must not retain a new transfer identity"
    );
    let writer = f.begin(1, b"x");
    f.end(1, 1).unwrap();
    f.events(true);
    f.export.authority_mut().generation = 10;
    assert!(
        f.export.take_execution().is_none(),
        "a queued payload must not outlive its source"
    );
    assert!(matches!(
        f.events(true).last().unwrap().1,
        PortalFileEvent::TransferOutcome(PortalTransferOutcome {
            state: PortalGrantState::Revoked,
            ..
        })
    ));
    f.export.release(Node::Slot, Some(writer));
}

#[test]
fn acknowledged_submission_cannot_be_replayed_or_replace_the_watermark() {
    let mut f = Fixture::new(PortalFileLimits::defaults(1));
    f.negotiate();
    f.request(1);
    f.next = 2;
    assert_eq!(f.send(request(1)), Err(Errno(114)));
    f.next = 1;
    assert_eq!(f.send(request(2)), Err(Errno(114)));
    assert!(f.events(false).is_empty());
    assert!(f.export.take_execution().is_none());
}

#[test]
fn lifecycle_atomic_decision_refusal_does_not_leave_pending_custody() {
    use sophia_portal::{PortalLifecycleError, PortalPolicyDecision, PortalRequestGrantLifecycle};
    let mut lifecycle = PortalRequestGrantLifecycle::with_capacity(1, 1).unwrap();
    let PortalFileCandidate::TransferRequest(request) = request(1) else {
        unreachable!()
    };
    assert_eq!(
        lifecycle.submit_decided(request.clone(), PortalPolicyDecision::Allow, 10, 100),
        Err(PortalLifecycleError::StaleSourceGeneration)
    );
    assert!(lifecycle.request(request.transfer.transfer).is_none());
    assert!(
        lifecycle
            .submit_decided(request, PortalPolicyDecision::Allow, 9, 100)
            .unwrap()
            .is_some()
    );
}

#[test]
fn security_revision_change_fences_queued_work_even_when_live_again() {
    let mut f = Fixture::new(PortalFileLimits::defaults(1));
    f.negotiate();
    assert!(matches!(
        f.request(1),
        PortalBrokerResponseDecision::Allowed(_)
    ));
    let _writer = f.begin(1, b"pending");
    f.end(1, 7).unwrap();
    f.export.authority_mut().revision += 2;
    assert!(f.export.authority().live);
    assert!(
        f.export.take_execution().is_none(),
        "lock/unlock must revoke queued work"
    );
    assert!(matches!(
        f.export.open(&Node::Api, OpenFlags(0)),
        Err(Errno::ESTALE)
    ));
}
