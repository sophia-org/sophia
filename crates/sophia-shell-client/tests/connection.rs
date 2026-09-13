use std::io::{Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sophia_protocol::*;
use sophia_shell_client::*;

fn socket(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "sophia-shell-client-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut header = [0u8; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header).unwrap();
    let payload = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    let mut frame = header.to_vec();
    frame.resize(SOPHIA_IPC_HEADER_LEN + payload, 0);
    stream
        .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    frame
}

fn options(capabilities: u64) -> ShellClientOptions {
    ShellClientOptions {
        minimum_revision: 5,
        maximum_revision: 6,
        required_capabilities: capabilities,
        handshake_timeout: Duration::from_secs(1),
    }
}

fn welcome(capabilities: u64) -> ShellV1ServerWelcome {
    ShellV1ServerWelcome {
        selected_revision: 6,
        connection_epoch: 7,
        capabilities,
        max_descriptors: 16,
        max_label_bytes: 128,
        max_pending_activations: 16,
    }
}

#[test]
fn content_connection_negotiates_and_enforces_direction() {
    let path = socket("content");
    let listener = UnixListener::bind(&path).unwrap();
    let capabilities =
        SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let hello = decode_shell_v1_client_hello_frame(&read_frame(&mut stream)).unwrap();
        assert_eq!(hello.required_capabilities, capabilities);
        stream
            .write_all(&encode_shell_v1_server_welcome_frame(welcome(capabilities)).unwrap())
            .unwrap();
        let limits = ContentLimits::prototype(ContentGrant {
            connection_epoch: 7,
            content_grant_epoch: 9,
        });
        stream
            .write_all(
                &encode_shell_content_frame(
                    TransactionId::INVALID,
                    &ShellContentRecord::Limits(limits.clone()),
                )
                .unwrap(),
            )
            .unwrap();
        let (transaction, record) = decode_shell_content_frame(&read_frame(&mut stream)).unwrap();
        assert_eq!(transaction, TransactionId::from_raw(3));
        assert!(matches!(record, ShellContentRecord::FrameDemand(_)));
    });
    let mut client = ShellConnection::connect(&path, options(capabilities)).unwrap();
    let (_, record) = loop {
        if let Some(record) = client.poll_content().unwrap() {
            break record;
        }
        std::thread::yield_now();
    };
    let ShellContentRecord::Limits(limits) = record else {
        panic!("expected content limits");
    };
    assert_eq!(limits.grant.content_grant_epoch, 9);
    let demand = ShellContentRecord::FrameDemand(ContentFrameDemand {
        grant: limits.grant,
        output: ContentOutputId {
            id: 1,
            generation: 1,
        },
        allocation: ContentAllocationId::default(),
        demand_id: 1,
        reason: 1,
    });
    client
        .send_content(TransactionId::from_raw(3), &demand)
        .unwrap();
    assert_eq!(
        client.send_content(
            TransactionId::from_raw(4),
            &ShellContentRecord::Limits(limits)
        ),
        Err(ShellClientError::WrongDirection)
    );
    server.join().unwrap();
}

#[test]
fn explicit_content_refusal_is_not_reported_as_corrupt_io() {
    let path = socket("refusal");
    let listener = UnixListener::bind(&path).unwrap();
    let capabilities =
        SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        decode_shell_v1_client_hello_frame(&read_frame(&mut stream)).unwrap();
        stream
            .write_all(
                &encode_shell_content_frame(
                    TransactionId::INVALID,
                    &ShellContentRecord::AdmissionRefused(ContentAdmissionRefused {
                        reason: ContentReason::Unauthorized as u16,
                        denied_capabilities: SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
                    }),
                )
                .unwrap(),
            )
            .unwrap();
    });
    assert_eq!(
        ShellConnection::connect(&path, options(capabilities)).err(),
        Some(ShellClientError::AdmissionRefused(
            ContentAdmissionRefused {
                reason: ContentReason::Unauthorized as u16,
                denied_capabilities: SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
            }
        ))
    );
    server.join().unwrap();
}

#[test]
fn indicator_publications_and_activation_outcomes_share_the_connection_without_reordering_content()
{
    let path = socket("indicator-content");
    let listener = UnixListener::bind(&path).unwrap();
    let capabilities = SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
        | SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS
        | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION;
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        decode_shell_v1_client_hello_frame(&read_frame(&mut stream)).unwrap();
        stream
            .write_all(&encode_shell_v1_server_welcome_frame(welcome(capabilities)).unwrap())
            .unwrap();
        let limits = ContentLimits::prototype(ContentGrant {
            connection_epoch: 7,
            content_grant_epoch: 9,
        });
        stream
            .write_all(
                &encode_shell_content_frame(
                    TransactionId::INVALID,
                    &ShellContentRecord::Limits(limits.clone()),
                )
                .unwrap(),
            )
            .unwrap();
        let snapshot = ShellIndicatorSnapshot {
            connection_epoch: 7,
            generation: 4,
            active_output: Some(OutputId::from_raw(2)),
            statuses: vec![],
            indicators: vec![ShellIndicator {
                output: OutputId::from_raw(2),
                indicator: 3,
                action: 5,
                slot: 0,
                state_bits: 1,
                label: "1".into(),
            }],
        };
        for frame in encode_shell_indicator_snapshot(TransactionId::from_raw(8), &snapshot).unwrap()
        {
            stream.write_all(&frame).unwrap();
        }
        let (transaction, activation) =
            decode_shell_indicator_activation(&read_frame(&mut stream)).unwrap();
        assert_eq!(transaction, TransactionId::from_raw(9));
        assert_eq!(activation.action, 5);
        stream
            .write_all(
                &encode_shell_indicator_activation_outcome(
                    transaction,
                    &ShellIndicatorActivationOutcome {
                        connection_epoch: 7,
                        snapshot_generation: 4,
                        event_id: activation.event_id,
                        status: ShellIndicatorActivationStatus::Accepted,
                        reason: 0,
                    },
                )
                .unwrap(),
            )
            .unwrap();
    });

    let mut client = ShellConnection::connect(&path, options(capabilities)).unwrap();
    let (_, limits_record) = loop {
        if let Some(record) = client.poll_content().unwrap() {
            break record;
        }
        std::thread::yield_now();
    };
    assert!(matches!(limits_record, ShellContentRecord::Limits(_)));
    let (_, snapshot) = loop {
        if let Some(snapshot) = client.poll_indicators().unwrap() {
            break snapshot;
        }
        std::thread::yield_now();
    };
    assert_eq!(snapshot.active_output, Some(OutputId::from_raw(2)));
    assert_eq!(snapshot.indicators[0].label, "1");
    client
        .send_indicator_activation(
            TransactionId::from_raw(9),
            &ShellIndicatorActivation {
                connection_epoch: 7,
                snapshot_generation: 4,
                output: OutputId::from_raw(2),
                indicator: 3,
                action: 5,
                event_id: 11,
            },
        )
        .unwrap();
    let (_, outcome) = loop {
        if let Some(outcome) = client.poll_indicator_activation_outcome().unwrap() {
            break outcome;
        }
        std::thread::yield_now();
    };
    assert_eq!(outcome.status, ShellIndicatorActivationStatus::Accepted);
    server.join().unwrap();
}
