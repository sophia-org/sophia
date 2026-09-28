//! The content conformance host over 9P2000.L with the independent C peer.
//!
//! Every run here is device-free: the host launches the peer in its protected
//! bubblewrap domain and supplies renderer failure instead of presentation. No
//! display, DRM node or TTY is opened, so nothing here claims GPU execution or
//! native presentation.
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_protocol::{
    ContentAllocationId, ContentLogicalRect, ContentMargins, ContentOutputFactsEntry,
    ContentOutputId, ContentPixelRect, TransactionId,
};
use sophia_runtime::{
    ContentAllocationSnapshot, ContentEpochRegistry, ProtectionBackendKind,
    ProtectionDomainEvidence, ProtectionDomainRole, ShellComponentTransport,
    ShellContentAdmissionPolicy, ShellTransportError,
};

#[path = "support/c_content_peer.rs"]
mod c_content_peer;

const HOST: &str = env!("CARGO_BIN_EXE_shell_content_conformance_host");
const HOST_RECORD: &str = "sophia_shell_content_transport schema=1 status=complete protected=true wire=9p2000.L allocation=granted bytes=8 accepted=true candidate=accepted renderer_outcome=9 lease_retained=true released=true native_presentation=false transaction=1";

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "shell-content-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run to completion within a bounded wait; the host's own deadline is 5 s.
fn bounded(command: &mut Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("content host exceeded the bounded wait");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    Output {
        status,
        stdout,
        stderr,
    }
}

fn host(args: &[&std::ffi::OsStr]) -> Command {
    let mut command = Command::new(HOST);
    command.args(args).env_remove("SOPHIA_SHELL_SOCKET");
    command
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_host_completes_with_the_independent_c_peer_over_9p() {
    let scratch = Scratch::new();
    let peer = c_content_peer::build(&scratch.0, 0);
    for transport in [None, Some("--transport=9p2000.L")] {
        let mut args = vec![peer.as_os_str()];
        args.extend(transport.map(std::ffi::OsStr::new));
        let output = bounded(&mut host(&args));
        let stdout = text(&output.stdout);
        assert!(
            output.status.success(),
            "{transport:?}: {stdout}{}",
            text(&output.stderr)
        );
        assert!(
            stdout.contains("c_content_files schema=1 status=complete wire=9p2000.L protected_socket=true native_presentation=false"),
            "{stdout}"
        );
        assert!(stdout.lines().any(|line| line == HOST_RECORD), "{stdout}");
    }
}

/// Each mutation is a separately built peer; the host must refuse every one
/// before emitting its completion record.
#[test]
fn the_host_refuses_each_red_mutation_of_the_peer() {
    let scratch = Scratch::new();
    for (mutation, expected) in [
        (1, "independent client uploaded different canonical pixels"),
        (2, "independent client changed its panel allocation request"),
        // Leaving before the retire surfaces from the owner as a lost peer.
        (3, "NotConnected"),
    ] {
        let peer = c_content_peer::build(&scratch.0, mutation);
        let output = bounded(&mut host(&[peer.as_os_str()]));
        let stderr = text(&output.stderr);
        assert!(!output.status.success(), "mutation {mutation} accepted");
        assert!(stderr.contains(expected), "mutation {mutation}: {stderr}");
        assert!(
            !text(&output.stdout).contains("status=complete protected=true"),
            "mutation {mutation} emitted completion"
        );
    }
}

#[test]
fn the_host_refuses_the_retired_socket_selection_and_variable() {
    let scratch = Scratch::new();
    let peer = c_content_peer::build(&scratch.0, 0);
    for (args, expected) in [
        (
            vec![peer.as_os_str(), "--transport=current-ipc".as_ref()],
            "current-ipc content host is retired",
        ),
        (
            vec![peer.as_os_str(), "--transport=auto".as_ref()],
            "usage: shell_content_conformance_host CLIENT [--transport=9p2000.L]",
        ),
        (
            vec![
                peer.as_os_str(),
                "--transport=9p2000.L".as_ref(),
                "extra".as_ref(),
            ],
            "unexpected content host argument",
        ),
        (
            vec!["relative/client".as_ref()],
            "shell client must be an absolute executable path",
        ),
    ] {
        let output = bounded(&mut host(&args));
        assert!(!output.status.success(), "{args:?}");
        assert!(text(&output.stderr).contains(expected), "{args:?}");
    }
    let output = bounded(host(&[peer.as_os_str()]).env("SOPHIA_SHELL_SOCKET", "/run/retired.sock"));
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("SOPHIA_SHELL_SOCKET selects the retired socket wire"));
}

/// The C peer stages records it encoded by hand from the KDL offsets. Each
/// malformed one must be refused at the export boundary with the contract's
/// errno and reach no owner; the same attach then still negotiates, allocates,
/// uploads, ends and retires with valid hand-encoded records.
#[test]
fn malformed_file_records_are_refused_before_any_owner() {
    let scratch = Scratch::new();
    let peer = c_content_peer::build(&scratch.0, 0);
    let mut owner = ShellComponentTransport::bind_for_supervised_uid(
        scratch.0.join("socket"),
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    let mut epochs = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let mut child = Command::new(&peer)
        .arg("content-malformed")
        .arg("--socket")
        .arg(owner.socket_path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Admission is by peer identity; this test's subject is the record
    // boundary, and the protected launch is covered by the host test above.
    owner
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: child.id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    owner
        .begin_file_negotiation(
            &epochs,
            1,
            Duration::from_secs(5),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    while owner
        .poll_negotiation(&mut epochs, 64 * 1024)
        .unwrap()
        .is_none()
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    let output = ContentOutputId {
        id: 2,
        generation: 1,
    };
    let mut requests = Vec::new();
    let mut demands = 0;
    let started = Instant::now();
    let status = {
        let mut transport = owner.connection(&mut epochs);
        transport
            .publish_content_output_facts(
                TransactionId::from_raw(2),
                1,
                vec![ContentOutputFactsEntry {
                    output,
                    local_width: 64,
                    local_height: 64,
                    scale_numerator: 1,
                    scale_denominator: 1,
                    scale_generation: 1,
                }],
            )
            .unwrap();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "control peer deadline"
            );
            let now = started.elapsed().as_millis() as u64;
            for result in [
                transport.service_content_resources(now).map(|_| ()),
                transport
                    .service_content_allocation_requests(&[], now)
                    .map(|_| ()),
            ] {
                match result {
                    Ok(()) | Err(ShellTransportError::NotConnected) => {}
                    Err(error) => panic!("owner service failed: {error}"),
                }
            }
            while let Some((_, request)) = transport.next_content_allocation_request() {
                let id = request.allocation_request_id;
                requests.push(request);
                transport
                    .grant_content_allocation(
                        id,
                        ContentAllocationSnapshot {
                            native_opening: None,
                            output,
                            allocation: ContentAllocationId {
                                id: 1,
                                generation: 1,
                            },
                            scale_generation: 1,
                            scale_numerator: 1,
                            scale_denominator: 1,
                            role: 1,
                            edge: 1,
                            margins: ContentMargins::default(),
                            logical: ContentLogicalRect {
                                x: 0,
                                y: 0,
                                width: 64,
                                height: 32,
                            },
                            pixel: ContentPixelRect {
                                x: 0,
                                y: 0,
                                width: 64,
                                height: 32,
                            },
                            parent: ContentAllocationId::default(),
                            anchor_parent_rect: ContentPixelRect::default(),
                            allowed_reservation_extent: 32,
                        },
                        &[],
                    )
                    .unwrap();
            }
            let allocations = transport.content_allocation_snapshots();
            match transport.service_content_demands(&[output], &allocations) {
                Ok(_) | Err(ShellTransportError::NotConnected) => {}
                Err(error) => panic!("demand service failed: {error}"),
            }
            while transport.next_content_demand().is_some() {
                demands += 1;
            }
            std::thread::sleep(Duration::from_micros(200));
        }
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(status.success(), "{stdout}{stderr}");
    for control in [
        "declared_length_below_written refused errno=22",
        "declared_length_above_transaction_cap refused errno=22",
        "submit_length_mismatch refused errno=22",
        "submit_reserved_nonzero refused errno=22",
        "api_version_2 refused errno=22",
        "event_kind_as_candidate refused errno=22",
        "unknown_kind refused errno=22",
        "submission_id_mismatch refused errno=22",
        "header_epoch_stale refused errno=116",
        "submit_epoch_stale refused errno=116",
        "allocation_reserved_nonzero refused errno=22",
        "allocation_operation_out_of_range refused errno=22",
        "replayed_submission_id refused errno=114",
        "upload_gap refused errno=22",
        "upload_past_declared_length refused errno=22",
    ] {
        assert!(
            stdout
                .lines()
                .any(|line| line == format!("control {control}")),
            "{control}: {stdout}"
        );
    }
    assert!(stdout.contains(
        "c_content_files_controls schema=1 status=complete wire=9p2000.L refused=15 accepted_after_refusal=true"
    ));
    // Only the one valid, independently encoded request reached the owner,
    // exactly as encoded; no refused record produced any owner input.
    assert_eq!(requests.len(), 1, "{requests:?}");
    let request = &requests[0];
    assert_eq!(request.allocation_request_id, 7);
    assert_eq!(request.output, output);
    assert_eq!(
        (
            request.operation,
            request.role,
            request.edge,
            request.desired_width,
            request.desired_height
        ),
        (1, 1, 1, 64, 32)
    );
    assert_eq!(demands, 0);
    // The accepted resource was retired and released: nothing remains.
    let transport = owner.connection(&mut epochs);
    assert_eq!(
        transport.content_usage().unwrap_or_default(),
        Default::default()
    );
    owner.disconnect(&mut epochs).unwrap();
    epochs.collect();
}
