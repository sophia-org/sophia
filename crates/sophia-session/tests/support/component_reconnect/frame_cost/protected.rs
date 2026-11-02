//! Real supervised authentication; only the control link and pixel oracle are fixture work.
use super::*;
use crate::live_session::authenticated_shell_peer::{Peer, rust_peer};
use crate::live_session::tests::lock_provider_factotum::Fixture;
use peer::PeerLink;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

pub(super) struct Custody {
    pub(super) auth: Fixture,
    pub(super) wake: sophia_wake::Wake,
    _bootstrap: Peer,
}

fn line(reader: &mut BufReader<UnixStream>) -> String {
    let mut bytes = Vec::new();
    reader.take(65).read_until(b'\n', &mut bytes).unwrap();
    assert!(
        bytes.len() <= 64 && bytes.last() == Some(&b'\n'),
        "invalid fixture control line"
    );
    String::from_utf8(bytes).unwrap()
}

struct SocketLink(BufReader<UnixStream>);
impl SocketLink {
    fn request(&mut self, text: &str) {
        self.0.get_mut().write_all(text.as_bytes()).unwrap();
        assert_eq!(line(&mut self.0), "go\n");
    }
}
impl PeerLink for SocketLink {
    fn turn(&mut self, turn: PeerTurn) {
        self.request(match turn {
            PeerTurn::Measure => "measure\n",
            PeerTurn::Fill => "fill\n",
            PeerTurn::Filled => "filled\n",
        });
    }
    fn verify(&mut self, frame: u64, _: u32) {
        self.request(&format!("verify {frame}\n"));
    }
}

#[test]
#[ignore = "supervised child of the exact protected attribution test"]
fn protected_frame_child() {
    let value = |name| std::env::var(name).unwrap().parse::<u64>().unwrap();
    let height = u32::try_from(value("SOPHIA_TEST_FRAME_HEIGHT")).unwrap();
    let frames = value("SOPHIA_TEST_FRAME_COUNT");
    let warmup = value("SOPHIA_TEST_FRAME_WARMUP");
    let socket = PathBuf::from(std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").unwrap());
    let report = PathBuf::from(std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap());
    let (mut client, relay) = rust_peer::connect(10, wire::capabilities(0));
    let limits = wire::enable_image(&mut client, &socket, 2);
    client
        .send_content(
            TransactionId::from_raw(10),
            &ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                grant: limits.grant,
                output: OUTPUT,
                allocation_request_id: 1,
                operation: 1,
                role: 1,
                edge: 1,
                prior: ContentAllocationId::default(),
                parent: ContentAllocationId::default(),
                parent_presentation_epoch: 0,
                anchor_parent_rect: ContentPixelRect::default(),
                desired_width: 1920,
                desired_height: height,
                margins: ContentMargins::default(),
            }),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut facts = false;
    loop {
        if let Some((_, record)) = client.poll_content().unwrap() {
            match record {
                ShellContentRecord::OutputFacts(observed) => {
                    assert_eq!(observed.facts_generation, 1);
                    assert_eq!(observed.outputs.len(), 1);
                    assert_eq!(observed.outputs[0].output, OUTPUT);
                    facts = true;
                }
                ShellContentRecord::AllocationResult(result) => {
                    assert!(facts);
                    assert_eq!(result.allocation, ALLOCATION);
                    assert_eq!(result.pixel.width, 1920);
                    assert_eq!(result.pixel.height, height);
                    break;
                }
                other => panic!("unexpected allocation record: {other:?}"),
            }
        }
        assert!(Instant::now() < deadline, "allocation deadline");
        client.wait_for_io(Duration::from_millis(1)).unwrap();
    }
    let stream = UnixStream::connect(report.with_extension("control")).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut link = SocketLink(BufReader::new(stream));
    std::fs::write(report.with_extension("tmp"), b"allocated\n").unwrap();
    std::fs::rename(report.with_extension("tmp"), &report).unwrap();
    let client = peer_frames(client, limits.grant, height, frames, warmup, &mut link);
    link.0.get_mut().write_all(b"done\n").unwrap();
    // Keep both authenticated owners alive until the parent closes its retained link.
    let mut end = [0];
    assert_eq!(link.0.read(&mut end).unwrap(), 0);
    drop(client);
    drop(relay);
}

pub(super) fn start(
    h: &mut Harness,
    height: u32,
    frames: u64,
    warmup: u64,
    mut link: ThreadLink,
) -> (
    ComponentConnectionKey,
    std::thread::JoinHandle<PeerConnection>,
    Custody,
) {
    let mut auth = Fixture::new();
    let wake = sophia_wake::Wake::new().unwrap();
    h.owner.panel_limit = u16::try_from(height).unwrap();
    h.owner
        .processes
        .authenticate(
            0,
            crate::launch_authentication::LaunchPolicy {
                principal: 4,
                namespace: 2,
                audience: 4,
                role: sophia_runtime::ProtectionDomainRole::MetadataShell,
            },
            "bar",
            auth.coordinator.endpoint(),
        )
        .unwrap();
    let reports = h.root.join("reports");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&reports)
        .unwrap();
    let report = reports.join("frame");
    let listener = UnixListener::bind(report.with_extension("control")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let oracle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "child control connection deadline"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("child control accept: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut verified = 0;
        loop {
            match line(&mut reader).as_str() {
                "measure\n" => {
                    assert_eq!(verified, warmup);
                    link.turn(PeerTurn::Measure);
                    cost::register_thread(Role::Peer);
                }
                "fill\n" => link.turn(PeerTurn::Fill),
                "filled\n" => link.turn(PeerTurn::Filled),
                "done\n" => {
                    assert_eq!(verified, frames);
                    break;
                }
                text => {
                    let frame = text
                        .strip_prefix("verify ")
                        .unwrap()
                        .trim_end()
                        .parse::<u64>()
                        .unwrap();
                    assert_eq!(frame, verified + 1);
                    link.verify(frame, height);
                    verified = frame;
                }
            }
            reader.get_mut().write_all(b"go\n").unwrap();
        }
        PeerConnection::Protected(reader.into_inner())
    });
    let bootstrap = Peer::rust_child(
        "frame",
        "live_session::metadata_shell::component_session::reconnect_tests::frame_cost::protected::protected_frame_child",
    );
    let key = h
        .owner
        .processes
        .start(
            0,
            |_, socket| {
                let executable = std::env::current_exe().unwrap();
                Ok(sophia_runtime::ProcessLaunchSpec::new(&bootstrap.0)
                    .arg(&executable)
                    .arg(socket)
                    .arg(&report)
                    .env("SOPHIA_TEST_SHELL_ROLE", "bar")
                    .env("SOPHIA_TEST_FRAME_HEIGHT", height.to_string())
                    .env("SOPHIA_TEST_FRAME_COUNT", frames.to_string())
                    .env("SOPHIA_TEST_FRAME_WARMUP", warmup.to_string())
                    .protection_domain(
                        sophia_runtime::ProtectionDomainSpec::bubblewrap([
                            sophia_runtime::ProtectionDomainRole::MetadataShell,
                        ])
                        .unwrap()
                        .path(sophia_runtime::ProtectionPath::read_only(
                            socket.parent().unwrap(),
                        ))
                        .unwrap()
                        .path(sophia_runtime::ProtectionPath::read_write(&reports))
                        .unwrap()
                        .path(sophia_runtime::ProtectionPath::read_only(executable))
                        .unwrap(),
                    ))
            },
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let output = descriptor();
    let scene = sophia_backend_live::LiveProductionCpuScene::new(output.size);
    let mut transaction = 1_000_000;
    while !report.exists() {
        auth.coordinator.poll(Instant::now()).unwrap();
        let visit = h
            .owner
            .poll_authenticated(65536, &mut auth.coordinator, wake.notifier())
            .unwrap();
        assert!(
            visit.negotiations.iter().flatten().all(|(_, r)| r.is_ok()),
            "negotiation: {:?}",
            visit.negotiations
        );
        assert!(
            visit.processes.iter().all(Option::is_none),
            "child startup: {:?}",
            visit.processes
        );
        if h.owner.phase(key).unwrap() == ComponentConnectionPhase::Connected {
            h.owner
                .with_service(key, |service, transport| {
                    let ShellComponentService::Bar(bar) = service else {
                        panic!("bar required")
                    };
                    bar.content
                        .service_until_submission(
                            transport,
                            h.backend.runtime_mut(),
                            &scene,
                            None,
                            &[output],
                            &mut || {
                                transaction += 1;
                                Ok(TransactionId::from_raw(transaction))
                            },
                            None,
                        )
                        .unwrap();
                })
                .unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "protected peer allocation deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(std::fs::read_to_string(report).unwrap(), "allocated\n");
    (
        key,
        oracle,
        Custody {
            auth,
            wake,
            _bootstrap: bootstrap,
        },
    )
}
