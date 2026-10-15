//! Comparison-tree feasibility, not timing acceptance. Both wires feed the
//! current resource owner with identical typed records and supplied admission.
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{ShellClientError, ShellClientOptions, ShellConnection};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);
enum Client {
    Files(Box<ShellConnection>),
    Ipc(UnixStream),
}
impl Client {
    fn connect(path: &Path, files: bool) -> Self {
        let caps =
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
        if files {
            Self::Files(Box::new(
                ShellConnection::connect_files(
                    path,
                    ShellClientOptions {
                        minimum_revision: 5,
                        maximum_revision: 6,
                        required_capabilities: caps,
                        handshake_timeout: WAIT,
                    },
                )
                .unwrap(),
            ))
        } else {
            let mut stream = UnixStream::connect(path).unwrap();
            stream.set_read_timeout(Some(WAIT)).unwrap();
            stream.set_write_timeout(Some(WAIT)).unwrap();
            stream
                .write_all(
                    &encode_shell_v1_client_hello_frame(ShellV1ClientHello {
                        minimum_revision: 5,
                        maximum_revision: 6,
                        required_capabilities: caps,
                    })
                    .unwrap(),
                )
                .unwrap();
            let welcome = decode_shell_v1_server_welcome_frame(&frame(&mut stream)).unwrap();
            assert_eq!(welcome.selected_revision, 6);
            Self::Ipc(stream)
        }
    }
    fn send(&mut self, id: u64, record: &ShellContentRecord) {
        let tx = TransactionId::from_raw(id);
        match self {
            Self::Files(client) => {
                let deadline = Instant::now() + WAIT;
                loop {
                    match client.enqueue_content(tx, record) {
                        Ok(()) => break,
                        Err(ShellClientError::QueueSaturated) => {
                            client.poll_io().unwrap();
                            assert!(Instant::now() < deadline, "file send deadline");
                            std::thread::yield_now();
                        }
                        Err(error) => panic!("file enqueue: {error:?}"),
                    }
                }
                client.poll_io().unwrap();
            }
            Self::Ipc(stream) => stream
                .write_all(&encode_shell_content_frame(tx, record).unwrap())
                .unwrap(),
        }
    }
    fn receive(&mut self) -> ShellContentRecord {
        match self {
            Self::Ipc(stream) => decode_shell_content_frame(&frame(stream)).unwrap().1,
            Self::Files(client) => {
                let deadline = Instant::now() + WAIT;
                loop {
                    if let Some((_, record)) = client.poll_content().unwrap() {
                        return record;
                    }
                    assert!(Instant::now() < deadline, "file response deadline");
                    std::thread::yield_now();
                }
            }
        }
    }
}
fn frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header).unwrap();
    let size = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    assert!(size <= SOPHIA_IPC_MAX_PAYLOAD_LEN);
    let mut bytes = header.to_vec();
    bytes.resize(SOPHIA_IPC_HEADER_LEN + size, 0);
    stream
        .read_exact(&mut bytes[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    bytes
}

fn exchange(files: bool, width: u32, height: u32) -> Vec<ShellContentRecord> {
    let root = std::env::temp_dir().join(format!(
        "shell-comparator-{}-{files}-{width}-{height}",
        std::process::id()
    ));
    let mut host = ShellComponentTransport::bind_for_supervised_uid(
        &root,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    host.authorize_protected_peer(&ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: std::process::id(),
        peer_pid: std::process::id(),
        roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
    })
    .unwrap();
    let mut epochs = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    host.reserve_content(&mut epochs, ContentLimits::prototype(grant))
        .unwrap();
    let socket = host.socket_path().to_owned();
    let (done_tx, done_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let bytes = (0..width as usize * height as usize * 4)
        .map(|n| if n % 4 == 3 { 255 } else { (n % 128) as u8 })
        .collect::<Vec<_>>();
    let expected = bytes.clone();
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let client = std::thread::spawn(move || {
        let mut client = Client::connect(&socket, files);
        let first = client.receive();
        let ShellContentRecord::Limits(limits) = &first else {
            panic!("missing limits")
        };
        assert_eq!(limits.grant, grant);
        let chunk_bytes = (limits.max_chunk_bytes / (width * 4) * (width * 4)) as usize;
        let chunks = bytes.len().div_ceil(chunk_bytes) as u32;
        let mut report = vec![first];
        client.send(
            1,
            &ShellContentRecord::ResourceBegin(ContentResourceBegin {
                grant,
                resource,
                width_px: width,
                height_px: height,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: chunks,
                total_bytes: bytes.len() as u64,
            }),
        );
        let begin = client.receive();
        assert!(
            matches!(&begin, ShellContentRecord::ResourceStatus(s) if s.status == 1),
            "files={files} size={width}x{height} response={begin:?}"
        );
        report.push(begin);
        for (ordinal, chunk) in bytes.chunks(chunk_bytes).enumerate() {
            client.send(
                2 + ordinal as u64,
                &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                    grant,
                    resource,
                    ordinal: ordinal as u32,
                    offset: ordinal as u64 * chunk_bytes as u64,
                    bytes: chunk.to_vec(),
                }),
            );
        }
        client.send(
            1000,
            &ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant,
                resource,
                total_bytes: bytes.len() as u64,
                chunk_count: chunks,
            }),
        );
        let accepted = client.receive();
        assert!(
            matches!(&accepted, ShellContentRecord::ResourceStatus(s) if s.status == 2),
            "files={files} size={width}x{height} response={accepted:?}"
        );
        report.push(accepted);
        done_tx.send(()).unwrap();
        release_rx.recv_timeout(WAIT).unwrap();
        client.send(
            1001,
            &ShellContentRecord::ResourceRetire(ContentResourceRetire { grant, resource }),
        );
        let released = client.receive();
        assert!(matches!(&released, ShellContentRecord::ResourceReleased(r) if r.reason == 0));
        report.push(released);
        // Retirement must not make the same generation admissible again.
        client.send(
            1002,
            &ShellContentRecord::ResourceBegin(ContentResourceBegin {
                grant,
                resource,
                width_px: width,
                height_px: height,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: chunks,
                total_bytes: bytes.len() as u64,
            }),
        );
        let stale = client.receive();
        assert!(matches!(&stale, ShellContentRecord::ResourceStatus(s)
            if s.status == 3 && s.reason == ContentReason::Stale as u16));
        report.push(stale);
        done_tx.send(()).unwrap();
        release_rx.recv_timeout(WAIT).unwrap();
        report
    });
    let policy = ShellContentAdmissionPolicy::Granted {
        discrete_input: false,
    };
    if files {
        host.begin_file_negotiation(&epochs, 1, WAIT, policy)
            .unwrap();
    } else {
        host.begin_negotiation(&epochs, 1, WAIT, policy).unwrap();
    }
    let start = Instant::now();
    while host.poll_negotiation(&mut epochs, 65536).unwrap().is_none() {
        assert!(start.elapsed() < WAIT);
        std::thread::yield_now();
    }
    let mut phase = 0;
    while phase < 2 {
        host.service_owner_turn(&mut epochs).unwrap();
        host.service_content_resources(&mut epochs, start.elapsed().as_millis() as u64)
            .unwrap();
        if done_rx.try_recv().is_ok() {
            if phase == 0 {
                let lease = host
                    .lease_content_resource(&epochs, grant, resource)
                    .unwrap();
                assert_eq!(lease.bytes(), expected);
                drop(lease);
                release_tx.send(()).unwrap();
            }
            phase += 1;
        }
        assert!(start.elapsed() < WAIT, "owner deadline");
        std::thread::yield_now();
    }
    host.disconnect(&mut epochs).unwrap();
    release_tx.send(()).unwrap();
    let report = client.join().unwrap();
    epochs.collect();
    assert!(epochs.accounting().quiescent());
    drop(host);
    if root.exists() {
        std::fs::remove_dir_all(root).unwrap();
    }
    report
}

#[test]
fn current_resource_owner_has_equal_results_on_both_wires() {
    for (width, height) in [(2, 1), (1920, 24), (1024, 1024)] {
        assert_eq!(
            exchange(true, width, height),
            exchange(false, width, height)
        );
    }
}
