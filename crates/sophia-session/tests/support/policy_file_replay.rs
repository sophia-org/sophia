//! Replay ownership uses typed events from a supplied fixture decoder. The
//! production array/scalar codecs have independent semantic/wire controls.
#![cfg(test)]
use super::super::adapter::{PolicyAdapter, PolicyProfileAdmission};
use super::super::driver::{PolicyReceiveKind, run_policy_transport};
use super::super::{PolicyTransportCommand, PolicyTransportEvent};
use super::custody_tests::{configuration_permit, record, submit};
use super::owner::{Handle, Node};
use super::*;
use sophia_protocol::*;
use std::sync::mpsc::sync_channel;

struct Codec;
fn event(kind: WmFileKind, tx: u64, epoch: u64) -> PolicyAdapterEvent {
    let transaction = TransactionId::from_raw(tx);
    match kind {
        WmFileKind::Configuration => PolicyAdapterEvent::Configuration {
            transaction,
            configuration: PolicyConfiguration {
                connection_epoch: epoch,
                generation: 3,
                actions: vec![],
                chrome: WmChromePolicy::default(),
            },
        },
        WmFileKind::Projection => {
            PolicyAdapterEvent::Projection(Box::new(PolicyProjectionProposal {
                transaction,
                connection_epoch: epoch,
                request_id: 3,
                base_generation: 2,
                active_output: OutputId::from_raw(1),
                outputs: vec![],
                indicators: vec![],
                output_statuses: vec![],
                tab_groups: vec![],
                translation_groups: vec![],
                launch_contexts: vec![],
                output_launch_contexts: vec![],
                presentation: None,
            }))
        }
        WmFileKind::SessionOperation => PolicyAdapterEvent::SessionOperation {
            transaction,
            request: PolicySessionOperationRequest {
                connection_epoch: epoch,
                request_id: 3,
                operation: 1,
                target: None,
            },
        },
        WmFileKind::Dirty => PolicyAdapterEvent::Dirty(PolicyDirtyRequest {
            connection_epoch: epoch,
            policy_generation: 3,
            affected_outputs: vec![OutputId::from_raw(1)],
        }),
        _ => panic!("fixture kind"),
    }
}
impl PolicyFileCodec for Codec {
    fn decode_candidate(&self, bytes: &[u8], _: u64) -> Result<DecodedFileCandidate, Errno> {
        let r = decode_wm_file_record(bytes, WmFileClass::Candidate).map_err(|_| Errno::EINVAL)?;
        if r.body.len() != 9 {
            return Err(Errno::EINVAL);
        }
        let tx = u64::from_le_bytes(r.body[..8].try_into().unwrap());
        Ok(DecodedFileCandidate {
            event: event(r.header.kind, tx, r.header.connection_epoch),
            required_capabilities: u64::from(r.body[8]),
        })
    }
    fn submitted_body(&self, id: u64, kind: WmFileKind) -> Result<Vec<u8>, Errno> {
        encode_wm_file_submitted_body(WmFileSubmitted {
            submission_id: id,
            candidate_kind: kind,
        })
        .map_err(|_| Errno::EINVAL)
    }
}
fn files(epoch: u64, qids: WmQids) -> WmFiles<Codec> {
    WmFiles::new(
        epoch,
        1,
        record(epoch, WmFileKind::Limits, 0, &[]),
        qids,
        Codec,
    )
    .unwrap()
}

// Only the existing driver creates permits. Script its normal configuration
// outcome and the corresponding control command, then retain the issued permit.
fn permit(kind: WmFileKind) -> PolicyReceivePermit {
    if kind == WmFileKind::Configuration {
        return configuration_permit();
    }
    struct Capture {
        kind: PolicyReceiveKind,
        captured: Option<PolicyReceivePermit>,
    }
    impl PolicyAdapter for Capture {
        fn admit(&mut self, _: u64, _: Option<PolicyProfileAdmission>) -> Result<(), String> {
            Ok(())
        }
        fn selected_capabilities(&self) -> u64 {
            1
        }
        fn receive_within(
            &mut self,
            p: PolicyReceivePermit,
            _: Duration,
        ) -> Result<PolicyAdapterEvent, String> {
            if p.kind() == self.kind {
                self.captured = Some(p);
                return Err("captured driver permit".into());
            }
            assert_eq!(p.kind(), PolicyReceiveKind::Configuration);
            Ok(event(WmFileKind::Configuration, 1, 9))
        }
        fn try_receive(
            &mut self,
            p: PolicyReceivePermit,
        ) -> Result<Option<PolicyAdapterEvent>, String> {
            assert_eq!(p.kind(), self.kind);
            self.captured = Some(p);
            Err("captured idle permit".into())
        }
        fn send(&mut self, _: &PolicyTransportCommand) -> Result<(), String> {
            Ok(())
        }
        fn disconnect(&mut self) {}
    }
    let kind = match kind {
        WmFileKind::Projection => PolicyReceiveKind::Projection { allow_dirty: true },
        WmFileKind::SessionOperation => PolicyReceiveKind::SessionOperation,
        WmFileKind::Dirty => PolicyReceiveKind::DirtyOnly,
        _ => panic!("fixture kind"),
    };
    let mut capture = Capture {
        kind,
        captured: None,
    };
    let (commands, receive) = sync_channel(2);
    commands
        .send(PolicyTransportCommand::ConfigurationOutcome {
            transaction: TransactionId::from_raw(1),
            generation: 3,
            outcome: PolicyProjectionOutcome::Committed,
        })
        .unwrap();
    match kind {
        PolicyReceiveKind::Projection { .. } => commands
            .send(PolicyTransportCommand::Cycle {
                snapshot_transaction: TransactionId::from_raw(2),
                request_transaction: TransactionId::from_raw(3),
                scene: Box::new(PolicySceneSnapshot {
                    generation: 2,
                    active_output: OutputId::from_raw(1),
                    outputs: vec![],
                    surfaces: vec![],
                    session_operations: vec![],
                }),
                actions: vec![],
                classifications: vec![],
                launch_origins: vec![],
                request: PolicyProjectionRequest {
                    connection_epoch: 9,
                    request_id: 3,
                    scene_generation: 2,
                    policy_generation: 3,
                    affected_outputs: vec![OutputId::from_raw(1)],
                    cause: PolicyRequestCause::SceneChanged,
                },
            })
            .unwrap(),
        PolicyReceiveKind::SessionOperation => commands
            .send(PolicyTransportCommand::ProjectionOutcome {
                transaction: TransactionId::from_raw(4),
                request_id: 3,
                scene_generation: 2,
                outcome: PolicyProjectionOutcome::Committed,
                expect_session_operation: true,
            })
            .unwrap(),
        _ => {}
    }
    let (events, _audit) = sync_channel::<PolicyTransportEvent>(8);
    assert!(run_policy_transport(&mut capture, 9, None, &receive, &events).is_err());
    capture.captured.unwrap()
}

fn stage(
    files: &mut WmFiles<Codec>,
    epoch: u64,
    id: u64,
    kind: WmFileKind,
    tx: u64,
    cap: u8,
) -> (Handle, Vec<u8>) {
    let bytes = record(
        epoch,
        kind,
        id,
        &[tx.to_le_bytes().as_slice(), &[cap]].concat(),
    );
    let mut handle = files.open(&Node::Transaction, OpenFlags(2)).unwrap();
    files
        .write(&Node::Transaction, &mut handle, 0, &bytes)
        .unwrap();
    (handle, submit(epoch, id, bytes.len()))
}
fn send(files: &mut WmFiles<Codec>, request: &[u8]) -> Result<u32, Errno> {
    files.write(&Node::Submit, &mut Handle::Plain, 0, request)
}
fn ack(files: &mut WmFiles<Codec>, epoch: u64, sequence: u64) {
    files
        .write(
            &Node::Ack,
            &mut Handle::Plain,
            0,
            &[epoch.to_le_bytes(), sequence.to_le_bytes()].concat(),
        )
        .unwrap();
}

#[test]
fn domain_replay_is_cross_kind_increasing_with_gaps_and_independent_of_submission_ids() {
    let mut owner = files(9, WmQids::new());
    for (sequence, id, kind, tx) in [
        (1, 10, WmFileKind::Configuration, 20),
        (2, 11, WmFileKind::Projection, 40),
        (3, 12, WmFileKind::SessionOperation, 90),
    ] {
        owner.offer(permit(kind)).unwrap();
        let (_, request) = stage(&mut owner, 9, id, kind, tx, 1);
        send(&mut owner, &request).unwrap();
        assert!(owner.take_delivery().is_some());
        // A retained exact retry does not need another permit or delivery.
        send(&mut owner, &request).unwrap();
        assert!(owner.take_delivery().is_none());
        ack(&mut owner, 9, sequence);
    }
    for (kind, tx) in [
        (WmFileKind::Configuration, 90),
        (WmFileKind::Projection, 40),
        (WmFileKind::SessionOperation, 89),
    ] {
        owner.offer(permit(kind)).unwrap();
        let (mut handle, request) = stage(&mut owner, 9, 13, kind, tx, 1);
        assert_eq!(send(&mut owner, &request), Err(EALREADY));
        assert!(
            matches!(owner.read(&Node::Transaction, &mut handle, 0, 100).unwrap(), ReadOutcome::Ready(bytes) if bytes.len() == 41)
        );
        assert!(owner.take_delivery().is_none());
        owner.release(Node::Transaction, Some(handle));
        owner.withdraw_permit();
    }
}

#[test]
fn a_later_rejected_outcome_and_its_ack_do_not_restore_a_consumed_domain_id() {
    let mut owner = files(9, WmQids::new());
    owner.offer(permit(WmFileKind::Configuration)).unwrap();
    let (_, request) = stage(&mut owner, 9, 1, WmFileKind::Configuration, 80, 1);
    send(&mut owner, &request).unwrap();
    assert!(owner.take_delivery().is_some());
    // Supply a real outcome record to the journal; this exercises file custody,
    // not the reducer's decision to reject a proposal.
    let bytes = encode_wm_file_configuration_outcome(
        WmFileHeader {
            kind: WmFileKind::ConfigurationOutcome,
            connection_epoch: 9,
            submission_id: 0,
            sequence: 2,
        },
        &WmFileConfigurationOutcome {
            transaction: TransactionId::from_raw(80),
            generation: 3,
            outcome: PolicyProjectionOutcome::RejectedInvalid,
        },
        SOPHIA_WM_CAPABILITY_CONFIGURATION,
    )
    .unwrap();
    let outcome = decode_wm_file_record(&bytes, WmFileClass::Event).unwrap();
    owner
        .append_event(outcome.header.kind, outcome.body)
        .unwrap();
    send(&mut owner, &request).unwrap();
    assert!(owner.take_delivery().is_none());
    ack(&mut owner, 9, 2);
    owner.offer(permit(WmFileKind::Configuration)).unwrap();
    let (_, request) = stage(&mut owner, 9, 2, WmFileKind::Configuration, 80, 1);
    assert_eq!(send(&mut owner, &request), Err(EALREADY));
    assert!(owner.take_delivery().is_none());
}

#[test]
fn dirty_uses_only_submission_custody_and_new_epoch_resets_domain_not_qid_identity() {
    let qids = WmQids::new();
    let mut owner = files(9, qids.clone());
    owner.offer(permit(WmFileKind::Configuration)).unwrap();
    let (_, request) = stage(&mut owner, 9, 1, WmFileKind::Configuration, 500, 1);
    send(&mut owner, &request).unwrap();
    owner.take_delivery();
    ack(&mut owner, 9, 1);
    for (id, sequence) in [(2, 2), (3, 3)] {
        owner.offer(permit(WmFileKind::Dirty)).unwrap();
        let (_, request) = stage(&mut owner, 9, id, WmFileKind::Dirty, 0, 1);
        send(&mut owner, &request).unwrap();
        assert!(matches!(
            owner.take_delivery(),
            Some(PolicyAdapterEvent::Dirty(_))
        ));
        send(&mut owner, &request).unwrap();
        assert!(owner.take_delivery().is_none());
        ack(&mut owner, 9, sequence);
    }
    owner.offer(permit(WmFileKind::Projection)).unwrap();
    let (_, request) = stage(&mut owner, 9, 4, WmFileKind::Projection, 500, 1);
    assert_eq!(send(&mut owner, &request), Err(EALREADY));
    let old_qid = owner.describe(&Node::Root, None).qid_path;
    owner.revoke();
    let mut fresh = files(10, qids);
    assert!(fresh.describe(&Node::Root, None).qid_path > old_qid);
    fresh.offer(permit(WmFileKind::Configuration)).unwrap();
    let (_, request) = stage(&mut fresh, 10, 1, WmFileKind::Configuration, 1, 1);
    send(&mut fresh, &request).unwrap();
    assert!(fresh.take_delivery().is_some());
}

#[test]
fn decode_capability_permit_and_credit_refusals_do_not_burn_domain_or_submission() {
    let mut owner = files(9, WmQids::new());
    let (handle, request) = stage(&mut owner, 9, 1, WmFileKind::Configuration, 80, 1);
    assert_eq!(send(&mut owner, &request), Err(Errno::EAGAIN));
    owner.release(Node::Transaction, Some(handle));
    owner.offer(permit(WmFileKind::Configuration)).unwrap();
    let (handle, request) = stage(&mut owner, 9, 1, WmFileKind::Projection, 80, 1);
    assert_eq!(send(&mut owner, &request), Err(Errno::EAGAIN));
    owner.release(Node::Transaction, Some(handle));
    let bytes = record(9, WmFileKind::Configuration, 1, &[]);
    let mut invalid = owner.open(&Node::Transaction, OpenFlags(2)).unwrap();
    owner
        .write(&Node::Transaction, &mut invalid, 0, &bytes)
        .unwrap();
    assert_eq!(
        send(&mut owner, &submit(9, 1, bytes.len())),
        Err(Errno::EINVAL)
    );
    owner.release(Node::Transaction, Some(invalid));
    let (handle, request) = stage(&mut owner, 9, 1, WmFileKind::Configuration, 80, 2);
    assert_eq!(send(&mut owner, &request), Err(Errno::EACCES));
    owner.release(Node::Transaction, Some(handle));
    for _ in 0..64 {
        owner.append_event(WmFileKind::Submitted, &[]).unwrap();
    }
    let (_, request) = stage(&mut owner, 9, 1, WmFileKind::Configuration, 80, 1);
    assert_eq!(send(&mut owner, &request), Err(Errno::EAGAIN));
    assert!(owner.take_delivery().is_none());
    ack(&mut owner, 9, 64);
    send(&mut owner, &request).unwrap();
    assert!(owner.take_delivery().is_some());
}
