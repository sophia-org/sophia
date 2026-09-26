//! Real adopted-stream startup and codec/reducer join; supplied peer admission,
//! not supervisor authentication, Hagia interoperability or runtime Cycle proof.
#![cfg(test)]
use super::*;
use crate::live_session::policy_transport_worker::{
    PolicyTransportCommand, PolicyTransportEvent,
    adapter::{PolicyAdapter, PolicyAdapterEvent},
    driver::run_policy_transport,
};
use sophia_protocol::*;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::mpsc::sync_channel;

#[allow(dead_code)]
#[path = "../../../sophia-protocol/tests/support/policy_record_fixture.rs"]
pub(in super::super) mod array_fixture;

pub(in super::super) fn admission() -> PolicyAdmissionPermit {
    struct Capture(Option<PolicyAdmissionPermit>);
    impl PolicyAdapter for Capture {
        fn admit(
            &mut self,
            token: PolicyAdmissionPermit,
            _: u64,
            _: Option<PolicyProfileAdmission>,
        ) -> Result<(), String> {
            self.0 = Some(token);
            Err("capture driver admission".into())
        }
        fn selected_capabilities(&self) -> u64 {
            0
        }
        fn receive_within(
            &mut self,
            _: PolicyReceivePermit,
            _: Duration,
        ) -> Result<PolicyAdapterEvent, String> {
            unreachable!()
        }
        fn try_receive(
            &mut self,
            _: PolicyReceivePermit,
        ) -> Result<Option<PolicyAdapterEvent>, String> {
            unreachable!()
        }
        fn send(&mut self, _: &PolicyTransportCommand) -> Result<(), String> {
            unreachable!()
        }
        fn disconnect(&mut self) {}
    }
    let mut capture = Capture(None);
    let (_commands, receive) = sync_channel(1);
    let (events, _audit) = sync_channel::<PolicyTransportEvent>(1);
    assert!(run_policy_transport(&mut capture, 9, None, &receive, &events).is_err());
    capture.0.unwrap()
}

pub(in super::super) struct Peer {
    stream: UnixStream,
    tag: u16,
    offset: u64,
    queued: VecDeque<Vec<u8>>,
}
impl Peer {
    pub(in super::super) fn from_stream(stream: UnixStream) -> Self {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        Self {
            stream,
            tag: 0,
            offset: 0,
            queued: VecDeque::new(),
        }
    }
    pub(in super::super) fn rpc(&mut self, kind: u8, body: &[u8]) -> io::Result<(u8, Vec<u8>)> {
        let tag = if kind == 100 {
            u16::MAX
        } else {
            self.tag += 1;
            self.tag
        };
        let mut bytes = ((7 + body.len()) as u32).to_le_bytes().to_vec();
        bytes.push(kind);
        bytes.extend(tag.to_le_bytes());
        bytes.extend(body);
        self.stream.write_all(&bytes)?;
        let mut header = [0; 7];
        self.stream.read_exact(&mut header)?;
        assert_eq!(u16::from_le_bytes(header[5..].try_into().unwrap()), tag);
        let size = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
        assert!((7..=65536).contains(&size));
        let mut body = vec![0; size - 7];
        self.stream.read_exact(&mut body)?;
        Ok((header[4], body))
    }
    fn setup(&mut self) {
        let version = [
            65536u32.to_le_bytes().as_slice(),
            &8u16.to_le_bytes(),
            b"9P2000.L",
        ]
        .concat();
        assert_eq!(self.rpc(100, &version).unwrap().0, 101);
        let attach = [
            1u32.to_le_bytes().as_slice(),
            &u32::MAX.to_le_bytes(),
            &[0; 4],
            &u32::MAX.to_le_bytes(),
        ]
        .concat();
        assert_eq!(self.rpc(104, &attach).unwrap().0, 105);
        self.open(2, b"events", 0);
        self.open(3, b"submit", 1);
        self.open(4, b"ack", 1);
    }
    pub(in super::super) fn open(&mut self, fid: u32, name: &[u8], mode: u32) {
        let walk = [
            1u32.to_le_bytes().as_slice(),
            &fid.to_le_bytes(),
            &1u16.to_le_bytes(),
            &(name.len() as u16).to_le_bytes(),
            name,
        ]
        .concat();
        assert_eq!(self.rpc(110, &walk).unwrap().0, 111);
        assert_eq!(
            self.rpc(12, &[fid.to_le_bytes(), mode.to_le_bytes()].concat())
                .unwrap()
                .0,
            13
        );
    }
    pub(in super::super) fn write(&mut self, fid: u32, bytes: &[u8]) -> io::Result<(u8, Vec<u8>)> {
        self.rpc(
            118,
            &[
                fid.to_le_bytes().as_slice(),
                &0u64.to_le_bytes(),
                &(bytes.len() as u32).to_le_bytes(),
                bytes,
            ]
            .concat(),
        )
    }
    pub(in super::super) fn submit(&mut self, bytes: &[u8]) -> io::Result<(u8, Vec<u8>)> {
        let record = decode_wm_file_record(bytes, WmFileClass::Candidate).unwrap();
        self.open(5, b"transaction", 2);
        assert_eq!(self.write(5, bytes)?.0, 119);
        let submit =
            super::super::custody_tests::submit(9, record.header.submission_id, bytes.len());
        self.write(3, &submit)
    }
    pub(in super::super) fn clear_transaction(&mut self) {
        assert_eq!(self.rpc(120, &5u32.to_le_bytes()).unwrap().0, 121);
    }
    pub(in super::super) fn next_event(&mut self) -> Vec<u8> {
        if let Some(bytes) = self.queued.pop_front() {
            return bytes;
        }
        let response = self
            .rpc(
                116,
                &[
                    2u32.to_le_bytes().as_slice(),
                    &self.offset.to_le_bytes(),
                    &65500u32.to_le_bytes(),
                ]
                .concat(),
            )
            .unwrap();
        assert_eq!(response.0, 117);
        let size = u32::from_le_bytes(response.1[..4].try_into().unwrap()) as usize;
        assert_eq!(size, response.1.len() - 4);
        self.offset += size as u64;
        let mut bytes = &response.1[4..];
        while !bytes.is_empty() {
            let size = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
            self.queued.push_back(bytes[..size].to_vec());
            bytes = &bytes[size..];
        }
        self.queued.pop_front().expect("nonempty read")
    }
    pub(in super::super) fn ack(&mut self, bytes: &[u8]) {
        let record = decode_wm_file_record(bytes, WmFileClass::Event).unwrap();
        assert_eq!(
            self.write(
                4,
                &[9u64.to_le_bytes(), record.header.sequence.to_le_bytes()].concat()
            )
            .unwrap()
            .0,
            119
        );
    }
}
pub(in super::super) fn header(kind: WmFileKind, id: u64) -> WmFileHeader {
    WmFileHeader {
        kind,
        connection_epoch: 9,
        submission_id: id,
        sequence: 0,
    }
}
fn pair(profile: bool, ceiling: u64) -> (FileStartup, Peer) {
    let (server, stream) = UnixStream::pair().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    (
        FileStartup::adopt(
            server,
            9,
            WmFileLimits {
                capability_ceiling: ceiling,
                profile_required: profile,
            },
            WmQids::new(),
        )
        .unwrap(),
        Peer {
            stream,
            tag: 0,
            offset: 0,
            queued: VecDeque::new(),
        },
    )
}
pub(in super::super) fn profile() -> PolicyProfileAdmission {
    PolicyProfileAdmission {
        connection_epoch: 9,
        generation: 3,
        digest: [7; 32],
        prepare_transaction: TransactionId::from_raw(40),
        activate_transaction: TransactionId::from_raw(41),
    }
}
pub(in super::super) fn negotiate(peer: &mut Peer, caps: u64) {
    peer.setup();
    let offer = encode_wm_file_negotiate(
        header(WmFileKind::Negotiate, 1),
        WmFileNegotiationOffer {
            required_capabilities: caps,
            optional_capabilities: 0,
        },
    )
    .unwrap();
    assert_eq!(peer.submit(&offer).unwrap().0, 119);
    // The retained submit retry is still valid while startup has moved on to
    // profile completion. It must not consume that permit or negotiate twice.
    let retry = super::super::custody_tests::submit(9, 1, offer.len());
    assert_eq!(peer.write(3, &retry).unwrap().0, 119);
    let submitted = peer.next_event();
    assert_eq!(
        decode_wm_file_submitted(&submitted).unwrap().submission_id,
        1
    );
    peer.ack(&submitted);
    peer.clear_transaction();
    let selected = peer.next_event();
    assert_eq!(decode_wm_file_negotiated(&selected).unwrap(), caps);
    peer.ack(&selected);
}

#[test]
fn supplied_stream_orders_negotiated_before_exact_profile_reducer_exchange() {
    let caps = SOPHIA_WM_CAPABILITY_CONFIGURATION | SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION;
    let (mut startup, mut peer) = pair(true, caps);
    let stop = startup.stop_handle();
    let (done, result) = sync_channel(1);
    let thread = std::thread::spawn(move || {
        let result = startup.admit(admission(), Some(profile()));
        done.send(result).unwrap();
        // Keep servicing the peer until it observes/ACKs the last custody event.
        while let Some(reactor) = startup.reactor.as_mut() {
            if reactor.turn(Duration::from_secs(1)).is_err() {
                break;
            }
        }
        startup
    });
    negotiate(&mut peer, caps);
    for (id, command_kind, completion_kind) in [
        (2, WmFileKind::ProfilePrepare, WmFileKind::ProfilePrepared),
        (3, WmFileKind::ProfileActivate, WmFileKind::ProfileActive),
    ] {
        let bytes = peer.next_event();
        let command = decode_wm_file_profile_command(&bytes, command_kind, caps).unwrap();
        assert_eq!(command.identity.profile_digest, [7; 32]);
        peer.ack(&bytes);
        let completion = encode_wm_file_profile_completion(
            header(completion_kind, id),
            PolicyProfileCompletion {
                transaction: command.transaction,
                identity: command.identity,
                outcome: PolicyProfileOutcome::Accepted,
            },
            caps,
        )
        .unwrap();
        assert_eq!(peer.submit(&completion).unwrap().0, 119);
        let submitted = peer.next_event();
        assert_eq!(
            decode_wm_file_submitted(&submitted).unwrap().submission_id,
            id
        );
        peer.ack(&submitted);
        peer.clear_transaction();
    }
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    stop.stop();
    let startup = thread.join().unwrap();
    assert_eq!(
        startup
            .reactor
            .unwrap()
            .server
            .export()
            .selected_capabilities(),
        Some(caps)
    );
}

#[test]
fn missing_required_dependency_closes_without_successful_negotiation() {
    let required = SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS;
    let (mut startup, mut peer) = pair(false, required);
    let thread = std::thread::spawn(move || {
        let result = startup.admit(admission(), None);
        (result, startup)
    });
    peer.setup();
    let bytes = encode_wm_file_negotiate(
        header(WmFileKind::Negotiate, 1),
        WmFileNegotiationOffer {
            required_capabilities: required,
            optional_capabilities: 0,
        },
    )
    .unwrap();
    // Cleanup may close before the successful custody Rwrite reaches the peer.
    let _ = peer.submit(&bytes);
    let (result, startup) = thread.join().unwrap();
    assert!(result.unwrap_err().contains("required capabilities"));
    assert!(startup.reactor.is_none());
}

#[test]
fn stop_wakes_offer_receive_and_closes_the_adopted_stream() {
    let (mut startup, _peer) = pair(false, 0);
    let stop = startup.stop_handle();
    let (done, result) = sync_channel(1);
    let thread = std::thread::spawn(move || {
        let result = startup.admit(admission(), None);
        done.send((result, startup.reactor.is_none())).unwrap();
    });
    stop.stop();
    let (result, closed) = result.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(result.is_err());
    assert!(closed);
    thread.join().unwrap();
}

#[test]
fn a_wrong_profile_identity_is_refused_by_the_existing_reducer_and_closes() {
    let caps = SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION;
    let (mut startup, mut peer) = pair(true, caps);
    let thread = std::thread::spawn(move || {
        let result = startup.admit(admission(), Some(profile()));
        (result, startup)
    });
    negotiate(&mut peer, caps);
    let bytes = peer.next_event();
    let command = decode_wm_file_profile_command(&bytes, WmFileKind::ProfilePrepare, caps).unwrap();
    peer.ack(&bytes);
    let completion = encode_wm_file_profile_completion(
        header(WmFileKind::ProfilePrepared, 2),
        PolicyProfileCompletion {
            transaction: TransactionId::from_raw(command.transaction.raw() + 1),
            identity: command.identity,
            outcome: PolicyProfileOutcome::Accepted,
        },
        caps,
    )
    .unwrap();
    let _ = peer.submit(&completion);
    let (result, startup) = thread.join().unwrap();
    assert!(result.is_err());
    assert!(startup.reactor.is_none());
}

#[test]
fn wrong_completion_kind_preserves_staging_and_stop_wakes_the_profile_receive() {
    let caps = SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION;
    let (mut startup, mut peer) = pair(true, caps);
    let stop = startup.stop_handle();
    let (done, result) = sync_channel(1);
    let thread = std::thread::spawn(move || {
        let result = startup.admit(admission(), Some(profile()));
        done.send((result, startup.reactor.is_none())).unwrap();
    });
    negotiate(&mut peer, caps);
    let bytes = peer.next_event();
    let command = decode_wm_file_profile_command(&bytes, WmFileKind::ProfilePrepare, caps).unwrap();
    peer.ack(&bytes);
    let completion = encode_wm_file_profile_completion(
        header(WmFileKind::ProfileActive, 2),
        PolicyProfileCompletion {
            transaction: command.transaction,
            identity: command.identity,
            outcome: PolicyProfileOutcome::Accepted,
        },
        caps,
    )
    .unwrap();
    let refused = peer.submit(&completion).unwrap();
    assert_eq!(refused, (7, 11u32.to_le_bytes().to_vec()));
    let retained = peer
        .rpc(
            116,
            &[
                5u32.to_le_bytes().as_slice(),
                &0u64.to_le_bytes(),
                &1000u32.to_le_bytes(),
            ]
            .concat(),
        )
        .unwrap();
    assert_eq!(retained.0, 117);
    assert_eq!(&retained.1[4..], completion.as_slice());
    assert!(result.try_recv().is_err());
    stop.stop();
    let (result, closed) = result.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(result.is_err());
    assert!(closed);
    thread.join().unwrap();
}

#[test]
fn wrong_header_epoch_is_refused_before_custody_without_closing_profile_wait() {
    let caps = SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION;
    let (mut startup, mut peer) = pair(true, caps);
    let stop = startup.stop_handle();
    let (done, result) = sync_channel(1);
    let thread = std::thread::spawn(move || {
        let result = startup.admit(admission(), Some(profile()));
        done.send((result, startup.reactor.is_none())).unwrap();
    });
    negotiate(&mut peer, caps);
    let bytes = peer.next_event();
    let command = decode_wm_file_profile_command(&bytes, WmFileKind::ProfilePrepare, caps).unwrap();
    peer.ack(&bytes);
    let completion = encode_wm_file_profile_completion(
        WmFileHeader {
            connection_epoch: 10,
            ..header(WmFileKind::ProfilePrepared, 2)
        },
        PolicyProfileCompletion {
            transaction: command.transaction,
            identity: PolicyProfileIdentity {
                connection_epoch: 10,
                ..command.identity
            },
            outcome: PolicyProfileOutcome::Accepted,
        },
        caps,
    )
    .unwrap();
    let refused = peer.submit(&completion).unwrap();
    assert_eq!(refused, (7, 116u32.to_le_bytes().to_vec()));
    let retained = peer
        .rpc(
            116,
            &[
                5u32.to_le_bytes().as_slice(),
                &0u64.to_le_bytes(),
                &1000u32.to_le_bytes(),
            ]
            .concat(),
        )
        .unwrap();
    assert_eq!(retained.0, 117);
    assert_eq!(&retained.1[4..], completion.as_slice());
    assert!(result.try_recv().is_err());
    stop.stop();
    let (result, closed) = result.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(result.is_err());
    assert!(closed);
    thread.join().unwrap();
}

#[test]
fn typed_codec_refuses_unnegotiated_sections_and_runtime_permits_refuse_startup() {
    let offered = encode_wm_file_negotiate(
        header(WmFileKind::Negotiate, 1),
        WmFileNegotiationOffer {
            required_capabilities: 0,
            optional_capabilities: 0,
        },
    )
    .unwrap();
    let event = TypedFileCodec.decode_candidate(&offered, 0).unwrap().event;
    let (_, profile_permission) = admission().negotiate();
    let model = sophia_runtime::PolicyProfileHandoffModel::new(
        PolicyProfileIdentity::new(9, 3, [7; 32]).unwrap(),
    );
    let update = sophia_runtime::reduce_policy_profile_handoff(
        &model,
        sophia_runtime::PolicyProfileHandoffMsg::Begin {
            kind: PolicyProfileHandoffKind::Prepare,
            transaction: TransactionId::from_raw(40),
        },
    )
    .unwrap();
    let effect = update.effect.unwrap();
    let completion = PolicyAdapterEvent::ProfileCompletion {
        kind: PolicyProfileHandoffKind::Prepare,
        completion: PolicyProfileCompletion {
            transaction: effect.command.transaction,
            identity: effect.command.identity,
            outcome: PolicyProfileOutcome::Accepted,
        },
    };
    for kind in [
        WmFileKind::Configuration,
        WmFileKind::Projection,
        WmFileKind::SessionOperation,
        WmFileKind::Dirty,
    ] {
        let permit = super::super::replay_tests::permit(kind);
        assert!(!permit.allows(&event));
        assert!(!permit.allows(&completion));
    }
    assert!(profile_permission.completion(effect).allows(&completion));
    let request = PolicyDirtyRequest {
        connection_epoch: 9,
        policy_generation: 1,
        affected_outputs: vec![OutputId::from_raw(1)],
    };
    let bytes = encode_wm_file_dirty(
        header(WmFileKind::Dirty, 2),
        &request,
        SOPHIA_WM_CAPABILITY_POLICY_DIRTY,
    )
    .unwrap();
    assert!(matches!(
        TypedFileCodec.decode_candidate(&bytes, 0),
        Err(Errno::EACCES)
    ));
    let proposal = array_fixture::proposal();
    let bytes = encode_wm_file_projection(
        WmFileHeader {
            connection_epoch: proposal.connection_epoch,
            ..header(WmFileKind::Projection, 3)
        },
        &proposal,
        u64::MAX,
    )
    .unwrap();
    assert!(TypedFileCodec.decode_candidate(&bytes, u64::MAX).is_ok());
    for capability in [
        SOPHIA_WM_CAPABILITY_INDICATORS,
        SOPHIA_WM_CAPABILITY_TAB_GROUPS,
        SOPHIA_WM_CAPABILITY_TRANSLATION_GROUPS,
        SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES,
        SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
    ] {
        assert!(matches!(
            TypedFileCodec.decode_candidate(&bytes, !capability),
            Err(Errno::EACCES)
        ));
    }
}

#[test]
fn canonical_selection_binds_once_within_ceiling_and_limits_remain_immutable() {
    let limits = WmFileLimits {
        capability_ceiling: SOPHIA_WM_CAPABILITY_CONFIGURATION,
        profile_required: false,
    };
    let mut owner =
        WmFiles::awaiting_negotiation(9, limits, WmQids::new(), TypedFileCodec).unwrap();
    use super::super::owner::{Handle, Node};
    let read_limits = |owner: &mut WmFiles<TypedFileCodec>| {
        let ReadOutcome::Ready(bytes) = owner
            .read(&Node::Limits, &mut Handle::Plain, 0, 1000)
            .unwrap()
        else {
            panic!("immutable limits")
        };
        bytes
    };
    let before = read_limits(&mut owner);
    assert_eq!(decode_wm_file_limits(&before).unwrap(), limits);
    assert_eq!(owner.selected_capabilities(), None);
    assert_eq!(
        owner.bind_selected(SOPHIA_WM_CAPABILITY_POLICY_DIRTY),
        Err(Errno::EACCES)
    );
    assert_eq!(owner.selected_capabilities(), None);
    owner.bind_selected(limits.capability_ceiling).unwrap();
    assert_eq!(owner.bind_selected(0), Err(EALREADY));
    assert_eq!(
        owner.selected_capabilities(),
        Some(limits.capability_ceiling)
    );
    assert_eq!(read_limits(&mut owner), before);
    owner.revoke();
    assert_eq!(owner.bind_selected(0), Err(Errno::ESTALE));
}
