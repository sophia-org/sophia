#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use sophia_protocol::{
    SOPHIA_IPC_HEADER_LEN, SOPHIA_WM_CAPABILITY_ACTIONS, SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
    SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES, WmV1ClientHello, decode_wm_v1_server_welcome_frame,
    encode_wm_v1_client_hello_frame,
};
use sophia_runtime::{
    PolicyPeerIdentity, PolicyTransferError, PolicyTransportError, PolicyWmSessionTransport,
};

#[test]
fn presentation_capabilities_require_an_admitted_execution_path() {
    let presentation =
        SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES | SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS;
    for (case, limit, expected) in [
        ("none", !presentation, 0),
        ("dependency", !SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES, 0),
        ("enabled", u64::MAX, presentation),
    ] {
        let directory = std::env::temp_dir().join(format!(
            "sophia-policy-capabilities-{}-{case}",
            std::process::id()
        ));
        let mut transport = PolicyWmSessionTransport::bind(
            &directory,
            PolicyPeerIdentity {
                uid: rustix::process::geteuid().as_raw(),
                pid: std::process::id(),
            },
        )
        .unwrap();
        transport.limit_capabilities(limit).unwrap();
        // A later caller cannot re-enable a mechanism this owner withheld.
        transport.limit_capabilities(u64::MAX).unwrap();
        let socket = transport.socket_path().to_owned();
        let client = std::thread::spawn(move || {
            let mut stream = UnixStream::connect(socket).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            stream
                .write_all(
                    &encode_wm_v1_client_hello_frame(&WmV1ClientHello {
                        minimum_revision: 3,
                        maximum_revision: 3,
                        capabilities: SOPHIA_WM_CAPABILITY_ACTIONS | presentation,
                    })
                    .unwrap(),
                )
                .unwrap();
            let mut header = [0; SOPHIA_IPC_HEADER_LEN];
            stream.read_exact(&mut header).unwrap();
            let length = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
            assert!(length < 128);
            let mut bytes = header.to_vec();
            bytes.resize(SOPHIA_IPC_HEADER_LEN + length, 0);
            stream
                .read_exact(&mut bytes[SOPHIA_IPC_HEADER_LEN..])
                .unwrap();
            let welcome = decode_wm_v1_server_welcome_frame(&bytes).unwrap();
            assert_eq!(welcome.capabilities & presentation, expected);
            assert_ne!(welcome.capabilities & SOPHIA_WM_CAPABILITY_ACTIONS, 0);
        });
        transport
            .accept_and_negotiate(1, Duration::from_secs(4))
            .unwrap();
        assert_eq!(transport.selected_capabilities() & presentation, expected);
        assert_eq!(
            transport.limit_capabilities(0),
            Err(PolicyTransportError::Transfer(
                PolicyTransferError::AlreadyConnected
            ))
        );
        client.join().unwrap();
        transport.disconnect().unwrap();
        drop(transport);
        assert!(!directory.exists());
    }
}
