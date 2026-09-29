//! Protected child for the process-custody and joined-launcher tests. Uses
//! the public SDK over the production 9P export; resource and owner assertions
//! are retained from the socket fixture at ef1f93a53.
use sophia_protocol::*;
use sophia_shell_client::{ShellClientOptions, ShellConnection};
use std::time::{Duration, Instant};

fn next_content(client: &mut ShellConnection) -> ShellContentRecord {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some((_, record)) = client.poll_content().unwrap() {
            return record;
        }
        assert!(Instant::now() < deadline, "missing content record");
        std::thread::sleep(Duration::from_millis(1));
    }
}
pub fn run() {
    let native = std::env::var("SOPHIA_FIXTURE_ROLE").unwrap() == "1";
    assert!(std::env::var_os("SOPHIA_SHELL_SOCKET").is_none());
    let capabilities = if native {
        SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
            | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER
    } else {
        SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    };
    let mut client = ShellConnection::connect_files(
        std::env::var_os("SOPHIA_SHELL_9P_SOCKET").unwrap(),
        ShellClientOptions {
            minimum_revision: if native { 7 } else { 6 },
            maximum_revision: if native { 7 } else { 6 },
            required_capabilities: capabilities,
            handshake_timeout: Duration::from_secs(5),
        },
    )
    .unwrap();
    let welcome = client.welcome();
    assert_eq!(welcome.selected_revision, if native { 7 } else { 6 });
    let ShellContentRecord::Limits(limits) = next_content(&mut client) else {
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
        client
            .send_content(TransactionId::from_raw(1), &record)
            .unwrap();
    }
    for expected in [1, 2] {
        let ShellContentRecord::ResourceStatus(status) = next_content(&mut client) else {
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
