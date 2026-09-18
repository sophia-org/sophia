use sophia_protocol::*;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

fn read(stream: &mut UnixStream) -> Vec<u8> {
    let mut bytes = vec![0; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut bytes).unwrap();
    let length = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    assert!(length <= 65536);
    bytes.resize(SOPHIA_IPC_HEADER_LEN + length, 0);
    stream
        .read_exact(&mut bytes[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    bytes
}
pub fn run() {
    let native = std::env::var("SOPHIA_FIXTURE_ROLE").unwrap() == "1";
    let mut stream =
        UnixStream::connect(std::env::var_os(sophia_runtime::SOPHIA_SHELL_SOCKET_ENV).unwrap())
            .unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let capabilities = if native {
        SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
            | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER
    } else {
        SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    };
    stream
        .write_all(
            &encode_shell_v1_client_hello_frame(ShellV1ClientHello {
                minimum_revision: if native { 7 } else { 6 },
                maximum_revision: if native { 7 } else { 6 },
                required_capabilities: capabilities,
            })
            .unwrap(),
        )
        .unwrap();
    let welcome = decode_shell_v1_server_welcome_frame(&read(&mut stream)).unwrap();
    assert_eq!(welcome.selected_revision, if native { 7 } else { 6 });
    let (_, ShellContentRecord::Limits(limits)) =
        decode_shell_content_frame(&read(&mut stream)).unwrap()
    else {
        panic!("limits");
    };
    assert_eq!(welcome.connection_epoch, limits.grant.connection_epoch);
    let grant = limits.grant;
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    for record in [
        ShellContentRecord::ResourceBegin(ContentResourceBegin {
            grant,
            resource,
            width_px: 1,
            height_px: 1,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 4,
        }),
        ShellContentRecord::ResourceChunk(ContentResourceChunk {
            grant,
            resource,
            ordinal: 0,
            offset: 0,
            bytes: vec![1, 2, 3, 255],
        }),
        ShellContentRecord::ResourceEnd(ContentResourceEnd {
            grant,
            resource,
            total_bytes: 4,
            chunk_count: 1,
        }),
    ] {
        stream
            .write_all(&encode_shell_content_frame(TransactionId::from_raw(1), &record).unwrap())
            .unwrap();
    }
    for expected in [1, 2] {
        let (_, ShellContentRecord::ResourceStatus(status)) =
            decode_shell_content_frame(&read(&mut stream)).unwrap()
        else {
            panic!("resource status");
        };
        assert_eq!(status.status, expected);
        assert_eq!(status.grant, grant);
    }
    std::thread::sleep(std::time::Duration::from_secs(10));
}

pub fn receive_resource(
    owner: &mut sophia_session::shell_component_processes::ShellComponentProcesses,
    key: sophia_session::shell_component_connections::ComponentConnectionKey,
) -> sophia_runtime::ContentResourceLease {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let visit = owner.visit(1024);
        assert!(visit.processes.iter().all(Option::is_none));
        for (_, result) in visit.negotiations.into_iter().flatten() {
            result.unwrap();
        }
        if owner.phase(key).unwrap()
            == sophia_session::shell_component_connections::ComponentConnectionPhase::Connected
        {
            let result = owner
                .with_connection(key, |connection| {
                    connection.service_content_resources(1).unwrap();
                    connection.poll_io().unwrap();
                    connection.lease_content_resource(
                        key.grant,
                        ContentResourceId {
                            id: 1,
                            generation: 1,
                        },
                    )
                })
                .unwrap();
            if let Ok(lease) = result {
                return lease;
            }
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
