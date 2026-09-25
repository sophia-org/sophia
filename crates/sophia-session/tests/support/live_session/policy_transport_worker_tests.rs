use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_protocol::{
    PolicyConfiguration, SOPHIA_IPC_HEADER_LEN, SOPHIA_WM_CAPABILITY_CONFIGURATION,
    SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION, TransactionId, WM_V1_PROFILE_DIGEST_BYTES,
    WmChromePolicy, WmV1ClientHello, WmV1ProfileCompletion, WmV1ProfileIdentity,
    WmV1ProfileOutcome, decode_wm_v1_profile_activate, decode_wm_v1_profile_prepare,
    decode_wm_v1_server_welcome_frame, encode_wm_v1_client_hello_frame,
    encode_wm_v1_policy_configuration, encode_wm_v1_policy_configuration_frame,
    encode_wm_v1_profile_active, encode_wm_v1_profile_prepared,
};
use sophia_runtime::{PolicyPeerIdentity, PolicyWmSessionTransport};

use super::super::policy_transport_worker::{PolicyTransportEvent, PolicyTransportWorker};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

#[test]
fn profile_admission_precedes_negotiation_and_configuration_events() {
    let (transport, socket_path) = bind_profile_transport("success");
    let identity = WmV1ProfileIdentity::new(9, 7, [0x5a; WM_V1_PROFILE_DIGEST_BYTES]).unwrap();
    let client = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(socket_path).unwrap();
        let capabilities =
            SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION | SOPHIA_WM_CAPABILITY_CONFIGURATION;
        stream
            .write_all(
                &encode_wm_v1_client_hello_frame(&WmV1ClientHello {
                    minimum_revision: 3,
                    maximum_revision: 3,
                    capabilities,
                })
                .unwrap(),
            )
            .unwrap();
        let welcome = decode_wm_v1_server_welcome_frame(&read_frame(&mut stream)).unwrap();
        assert_eq!(welcome.capabilities, capabilities);

        let prepare = decode_wm_v1_profile_prepare(&read_frame(&mut stream)).unwrap();
        assert_eq!(prepare.identity, identity);
        stream
            .write_all(
                &encode_wm_v1_profile_prepared(WmV1ProfileCompletion {
                    transaction: prepare.transaction,
                    identity: prepare.identity,
                    outcome: WmV1ProfileOutcome::Accepted,
                })
                .unwrap(),
            )
            .unwrap();
        let activate = decode_wm_v1_profile_activate(&read_frame(&mut stream)).unwrap();
        assert_eq!(activate.identity, identity);
        stream
            .write_all(
                &encode_wm_v1_profile_active(WmV1ProfileCompletion {
                    transaction: activate.transaction,
                    identity: activate.identity,
                    outcome: WmV1ProfileOutcome::Accepted,
                })
                .unwrap(),
            )
            .unwrap();

        let configuration = encode_wm_v1_policy_configuration(&PolicyConfiguration {
            connection_epoch: 9,
            generation: 1,
            actions: Vec::new(),
            chrome: WmChromePolicy::default(),
        })
        .unwrap();
        stream
            .write_all(
                &encode_wm_v1_policy_configuration_frame(
                    TransactionId::from_raw(1),
                    &configuration,
                )
                .unwrap(),
            )
            .unwrap();
        let mut closed = [0_u8; 1];
        assert_eq!(stream.read(&mut closed).unwrap(), 0);
    });

    let worker = PolicyTransportWorker::new_profile_activated(
        transport,
        9,
        identity,
        TransactionId::from_raw(1),
        TransactionId::from_raw(2),
    )
    .unwrap();
    assert!(matches!(
        next_event(&worker),
        PolicyTransportEvent::Negotiated
    ));
    assert!(matches!(
        next_event(&worker),
        PolicyTransportEvent::Configuration {
            transaction,
            configuration: PolicyConfiguration {
                connection_epoch: 9,
                generation: 1,
                ..
            },
        } if transaction == TransactionId::from_raw(1)
    ));
    drop(worker);
    client.join().unwrap();
}

#[test]
fn rejected_profile_admission_fails_before_negotiated() {
    let (transport, socket_path) = bind_profile_transport("rejected");
    let identity = WmV1ProfileIdentity::new(9, 7, [0x5a; WM_V1_PROFILE_DIGEST_BYTES]).unwrap();
    let client = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(socket_path).unwrap();
        stream
            .write_all(
                &encode_wm_v1_client_hello_frame(&WmV1ClientHello {
                    minimum_revision: 3,
                    maximum_revision: 3,
                    capabilities: SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
                })
                .unwrap(),
            )
            .unwrap();
        decode_wm_v1_server_welcome_frame(&read_frame(&mut stream)).unwrap();
        let prepare = decode_wm_v1_profile_prepare(&read_frame(&mut stream)).unwrap();
        stream
            .write_all(
                &encode_wm_v1_profile_prepared(WmV1ProfileCompletion {
                    transaction: prepare.transaction,
                    identity: prepare.identity,
                    outcome: WmV1ProfileOutcome::RejectedIdentity,
                })
                .unwrap(),
            )
            .unwrap();
        let mut closed = [0_u8; 1];
        assert_eq!(stream.read(&mut closed).unwrap(), 0);
    });

    let worker = PolicyTransportWorker::new_profile_activated(
        transport,
        9,
        identity,
        TransactionId::from_raw(1),
        TransactionId::from_raw(2),
    )
    .unwrap();
    let PolicyTransportEvent::Failed(error) = next_event(&worker) else {
        panic!("rejected profile admission must fail before negotiation")
    };
    assert!(error.contains("RejectedIdentity"));
    assert!(matches!(worker.try_event(), Err(())));
    client.join().unwrap();
}

fn bind_profile_transport(label: &str) -> (PolicyWmSessionTransport, PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "sophia-policy-worker-profile-{label}-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let peer = PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    };
    let transport =
        PolicyWmSessionTransport::bind_for_startup_profile_activation(&directory, peer).unwrap();
    let socket_path = transport.socket_path().to_path_buf();
    (transport, socket_path)
}

fn next_event(worker: &PolicyTransportWorker) -> PolicyTransportEvent {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(event) = worker
            .try_event()
            .expect("worker event channel remains open")
        {
            return event;
        }
        assert!(Instant::now() < deadline, "policy worker event timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut header = [0_u8; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header).unwrap();
    let payload_len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    let mut frame = header.to_vec();
    frame.resize(SOPHIA_IPC_HEADER_LEN + payload_len, 0);
    stream
        .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    frame
}
