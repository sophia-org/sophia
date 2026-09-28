//! A reloaded profile's output topology is Session's own transaction. It is
//! admitted with the output owner's epoch and settled locally; it is never
//! answered on the output client's connection as if that client had asked.
use super::*;
use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming,
    LibdrmNativeVrrPropertyDiscoveryStatus, project_live_output_authority_snapshot,
};
use sophia_protocol::{
    OutputAuthoritySnapshot, OutputHeadTargetProposal, OutputLogicalGroupProposal,
    OutputTopologyCandidate, OutputTopologyIntent, OutputTransform, OutputVrrPolicy,
};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

/// One connected head, its published snapshot and an apply candidate that
/// names it.
fn reload_inputs(
    public: &LivePublicPolicyState,
) -> (
    LibdrmNativeOutputCapability,
    OutputAuthoritySnapshot,
    OutputTopologyCandidate,
) {
    let output = public.outputs[0];
    let timing = LibdrmNativeOutputTiming::new(
        u32::try_from(output.size.width).unwrap(),
        u32::try_from(output.size.height).unwrap(),
        60_000,
    );
    let capability = LibdrmNativeOutputCapability::new(
        output.id,
        11,
        "DP-1",
        [timing],
        Some(timing),
        timing,
        LibdrmNativeVrrPropertyDiscoveryStatus::Discovered,
    )
    .unwrap()
    .bind_head(sophia_engine::RenderHeadId::from_raw(11))
    .unwrap();
    let snapshot =
        project_live_output_authority_snapshot(std::slice::from_ref(&capability), &[output], 7)
            .unwrap();
    let head = &snapshot.heads[0];
    let group = &snapshot.groups[0];
    let candidate = OutputTopologyCandidate {
        base_topology_epoch: snapshot.topology_epoch,
        intent: OutputTopologyIntent::Apply,
        primary_group_index: 0,
        heads: vec![OutputHeadTargetProposal {
            head: head.head,
            head_generation: head.generation,
            mode: head.current_mode.unwrap(),
            transform: OutputTransform::Normal,
            vrr: OutputVrrPolicy::Disabled,
        }],
        groups: vec![OutputLogicalGroupProposal {
            output: group.output,
            logical: group.logical,
            members: group.members.clone(),
        }],
    };
    (capability, snapshot, candidate)
}

/// The output role's connection epoch and the WM policy's connection epoch
/// advance independently: output on peer departure and assignee replacement,
/// the WM on policy restarts. A reload is admitted against the former in both
/// orders of divergence.
#[test]
fn output_reload_is_admitted_with_the_output_owner_epoch_not_the_wm_epoch() {
    for (output_epoch, wm_epoch) in [(3, 1), (1, 4)] {
        let mut fixture = ReloadFixture::new();
        let public = fixture.wm.public.as_mut().unwrap();
        let (capability, snapshot, candidate) = reload_inputs(public);
        public.connection_epoch = wm_epoch;
        public.output_authority = Some(
            crate::live_output_authority::LiveOutputAuthorityOwner::new(output_epoch, snapshot)
                .unwrap(),
        );
        public.output_capabilities = vec![capability];
        assert!(
            public.admit_reloaded_output_topology(candidate).unwrap(),
            "reload declined with output epoch {output_epoch} and WM epoch {wm_epoch}"
        );
        assert!(
            public
                .output_authority
                .as_ref()
                .unwrap()
                .active_transaction()
                .is_some()
        );
    }
}

fn read_output_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut header = [0; sophia_protocol::SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header).unwrap();
    let payload = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    let mut frame = header.to_vec();
    frame.resize(header.len() + payload, 0);
    stream.read_exact(&mut frame[header.len()..]).unwrap();
    frame
}

/// A connected output client keeps its service when Session settles its own
/// reload: the client receives no outcome for a transaction it never sent, the
/// service does not fail, and the client's next proposal is still admitted.
#[test]
fn output_reload_settlement_is_local_and_keeps_a_connected_client_served() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let (capability, snapshot, candidate) = reload_inputs(public);
    let directory = std::env::temp_dir().join(format!(
        "sophia-output-reload-settlement-{}",
        std::process::id()
    ));
    let transport = sophia_runtime::OutputSessionTransport::bind(
        &directory,
        sophia_runtime::PolicyPeerIdentity {
            uid: rustix::process::geteuid().as_raw(),
            pid: std::process::id(),
        },
    )
    .unwrap();
    let socket = transport.socket_path().to_owned();
    let service = sophia_runtime::OutputTransportService::spawn(
        transport,
        1,
        TransactionId::from_raw(1),
        snapshot.clone(),
    )
    .unwrap();
    let mut client = UnixStream::connect(&socket).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .write_all(
            &sophia_protocol::encode_output_v1_client_hello_frame(
                sophia_protocol::OutputV1ClientHello {
                    minimum_revision: 1,
                    maximum_revision: 1,
                    capabilities: sophia_protocol::SOPHIA_OUTPUT_CAPABILITY_OBSERVE
                        | sophia_protocol::SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
                },
            )
            .unwrap(),
        )
        .unwrap();
    sophia_protocol::decode_output_v1_server_welcome_frame(&read_output_frame(&mut client))
        .unwrap();
    sophia_protocol::decode_output_v1_snapshot_frame(&read_output_frame(&mut client)).unwrap();
    assert_eq!(
        service.event_timeout(Duration::from_secs(2)).unwrap(),
        sophia_runtime::OutputTransportServiceEvent::Connected {
            connection_epoch: 1
        }
    );
    public.output_service = Some(service);
    public.output_authority = Some(
        crate::live_output_authority::LiveOutputAuthorityOwner::new(1, snapshot.clone()).unwrap(),
    );
    public.output_capabilities = vec![capability];

    assert!(public.admit_reloaded_output_topology(candidate).unwrap());
    let transaction = public
        .take_output_topology_effect()
        .unwrap()
        .transaction;
    let authority = public.output_authority.as_mut().unwrap();
    authority
        .fail(sophia_engine::OutputTopologyTransactionFailure::Preparation)
        .unwrap();
    let settlement = authority.settle_terminal().unwrap();
    assert_eq!(settlement.transaction, transaction);
    public.finish_output_settlement(settlement).unwrap();
    assert!(!public.output_candidate_active());
    assert_eq!(public.published_output_snapshot(), Some(snapshot.clone()));

    // Nothing is written to the client for Session's transaction.
    client
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let mut byte = [0; 1];
    match client.read(&mut byte) {
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) => {}
        other => panic!("the client was sent something for Session's reload: {other:?}"),
    }
    let service = public.output_service.as_ref().expect("service retained");
    match service.event_timeout(Duration::from_millis(300)) {
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        other => panic!("output service reacted to Session's reload settlement: {other:?}"),
    }

    // The client's own proposal is still admitted on the same connection.
    let proposal = sophia_protocol::OutputV1Proposal {
        connection_epoch: 1,
        candidate: OutputTopologyCandidate {
            intent: OutputTopologyIntent::ValidateOnly,
            ..reload_inputs(public).2
        },
    };
    client
        .write_all(
            &sophia_protocol::encode_output_v1_proposal_frame(
                TransactionId::from_raw(2),
                &proposal,
            )
            .unwrap(),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "client proposal was not admitted");
        match public
            .output_service
            .as_ref()
            .unwrap()
            .event_timeout(Duration::from_millis(200))
        {
            Ok(sophia_runtime::OutputTransportServiceEvent::Proposal { proposal: admitted, .. }) => {
                assert_eq!(admitted.transaction, TransactionId::from_raw(2));
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            other => panic!("unexpected output service event {other:?}"),
        }
    }
    public.output_service.take();
    let _ = std::fs::remove_dir_all(directory);
}
