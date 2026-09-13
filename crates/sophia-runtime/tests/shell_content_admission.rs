use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sophia_protocol::{
    IpcMessageKind, SOPHIA_IPC_HEADER_LEN, SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT,
    SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE, SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER,
    ShellContentRecord, ShellV1ClientHello, decode_frame, decode_shell_content_frame,
    decode_shell_v1_server_welcome_frame, encode_shell_v1_client_hello_frame,
};
use sophia_runtime::{
    ProtectionBackendKind, ProtectionDomainEvidence, ProtectionDomainRole,
    ShellContentAdmissionPolicy, ShellSessionTransport, ShellTransportError,
};

fn directory(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "sophia-content-admission-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn evidence() -> ProtectionDomainEvidence {
    ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: std::process::id(),
        peer_pid: std::process::id(),
        roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
    }
}

fn hello(capabilities: u64) -> ShellV1ClientHello {
    ShellV1ClientHello {
        minimum_revision: 5,
        maximum_revision: 6,
        required_capabilities: capabilities,
    }
}

fn connect(path: std::path::PathBuf, capabilities: u64) -> UnixStream {
    connect_with_hello(path, hello(capabilities))
}

fn connect_with_hello(path: std::path::PathBuf, hello: ShellV1ClientHello) -> UnixStream {
    let mut stream = UnixStream::connect(path).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .write_all(&encode_shell_v1_client_hello_frame(hello).unwrap())
        .unwrap();
    stream
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

fn transport(label: &str) -> ShellSessionTransport {
    let mut transport = ShellSessionTransport::bind_for_supervised_uid(
        directory(label),
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    transport.authorize_protected_peer(&evidence()).unwrap();
    transport
}

#[test]
fn content_is_unavailable_without_an_explicit_service_policy() {
    let mut session = transport("unavailable");
    let socket = session.socket_path().to_path_buf();
    let client = std::thread::spawn(move || {
        let mut stream = connect(
            socket,
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
        );
        let frame = read_frame(&mut stream);
        assert_eq!(
            decode_frame(&frame).unwrap().0.message_kind,
            IpcMessageKind::ShellContentAdmissionRefused
        );
        let (_, ShellContentRecord::AdmissionRefused(refusal)) =
            decode_shell_content_frame(&frame).unwrap()
        else {
            panic!("expected content refusal");
        };
        assert_eq!(refusal.reason, 4);
        assert_eq!(
            refusal.denied_capabilities,
            SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
        );
        let mut byte = [0u8; 1];
        assert_eq!(stream.read(&mut byte).unwrap(), 0);
    });
    assert!(matches!(
        session.accept_and_negotiate(1, Duration::from_secs(2)),
        Err(ShellTransportError::ContentAdmissionRefused(refusal))
            if refusal.reason == 4
    ));
    client.join().unwrap();
}

#[test]
fn granted_content_gets_limits_and_a_fresh_epoch_on_replacement() {
    let mut session = transport("granted");
    let socket = session.socket_path().to_path_buf();
    let mut prior_grant_epoch = 0;
    for connection_epoch in [1, 2] {
        let client = std::thread::spawn({
            let socket = socket.clone();
            move || {
                let mut stream = connect(
                    socket,
                    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                        | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT,
                );
                let welcome =
                    decode_shell_v1_server_welcome_frame(&read_frame(&mut stream)).unwrap();
                let (_, ShellContentRecord::Limits(limits)) =
                    decode_shell_content_frame(&read_frame(&mut stream)).unwrap()
                else {
                    panic!("expected content limits");
                };
                (welcome, limits)
            }
        });
        let welcome = session
            .accept_and_negotiate_with_content_policy(
                connection_epoch,
                Duration::from_secs(2),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: true,
                },
            )
            .unwrap();
        assert!(session.supports_content());
        assert_eq!(session.content_reserved_bytes(), 40 * 1024 * 1024);
        assert_eq!(session.content_backing_reserved_bytes(), 32 * 1024 * 1024);
        let (client_welcome, limits) = client.join().unwrap();
        assert_eq!(client_welcome, welcome);
        assert_eq!(limits.grant.connection_epoch, connection_epoch);
        assert!(limits.grant.content_grant_epoch > prior_grant_epoch);
        prior_grant_epoch = limits.grant.content_grant_epoch;
        assert_eq!(session.content_grant(), Some(limits.grant));
        session.disconnect().unwrap();
        assert!(!session.supports_content());
        assert_eq!(session.content_reserved_bytes(), 0);
        assert_eq!(session.content_backing_reserved_bytes(), 0);
        if connection_epoch == 1 {
            session.authorize_protected_peer(&evidence()).unwrap();
        }
    }
}

#[test]
fn operator_denial_is_distinct_from_unavailable_implementation() {
    let mut session = transport("operator-denied");
    let socket = session.socket_path().to_path_buf();
    let client = std::thread::spawn(move || {
        let mut stream = connect(
            socket,
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
        );
        let (_, ShellContentRecord::AdmissionRefused(refusal)) =
            decode_shell_content_frame(&read_frame(&mut stream)).unwrap()
        else {
            panic!("expected content refusal");
        };
        refusal
    });
    assert!(matches!(
        session.accept_and_negotiate_with_content_policy(
            1,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Denied,
        ),
        Err(ShellTransportError::ContentAdmissionRefused(refusal))
            if refusal.reason == 1
    ));
    assert_eq!(client.join().unwrap().reason, 1);
}

#[test]
fn discrete_input_denial_names_only_that_required_capability() {
    let mut session = transport("input-denied");
    let socket = session.socket_path().to_path_buf();
    let client = std::thread::spawn(move || {
        let mut stream = connect(
            socket,
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT,
        );
        let (_, ShellContentRecord::AdmissionRefused(refusal)) =
            decode_shell_content_frame(&read_frame(&mut stream)).unwrap()
        else {
            panic!("expected content refusal");
        };
        refusal
    });
    let result = session.accept_and_negotiate_with_content_policy(
        1,
        Duration::from_secs(2),
        ShellContentAdmissionPolicy::Granted {
            discrete_input: false,
        },
    );
    assert!(matches!(
        result,
        Err(ShellTransportError::ContentAdmissionRefused(refusal))
            if refusal.reason == 1
                && refusal.denied_capabilities
                    == SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
    ));
    assert_eq!(
        client.join().unwrap().denied_capabilities,
        SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
    );
}

#[test]
fn invalid_content_dependencies_receive_no_revision_five_record() {
    let mut session = transport("invalid-dependency");
    let socket = session.socket_path().to_path_buf();
    let client = std::thread::spawn(move || {
        let mut stream = connect(
            socket,
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT,
        );
        let mut byte = [0u8; 1];
        assert_eq!(stream.read(&mut byte).unwrap(), 0);
    });
    assert_eq!(
        session.accept_and_negotiate_with_content_policy(
            1,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        ),
        Err(ShellTransportError::MissingCapability)
    );
    session.disconnect().unwrap();
    client.join().unwrap();
}

#[test]
fn pre_revision_five_peer_receives_no_content_record() {
    let mut session = transport("pre-content-revision");
    let socket = session.socket_path().to_path_buf();
    let client = std::thread::spawn(move || {
        let mut stream = connect_with_hello(
            socket,
            ShellV1ClientHello {
                minimum_revision: 4,
                maximum_revision: 4,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
            },
        );
        let mut byte = [0u8; 1];
        assert_eq!(stream.read(&mut byte).unwrap(), 0);
    });
    assert_eq!(
        session.accept_and_negotiate_with_content_policy(
            1,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        ),
        Err(ShellTransportError::MissingCapability)
    );
    session.disconnect().unwrap();
    client.join().unwrap();
}
