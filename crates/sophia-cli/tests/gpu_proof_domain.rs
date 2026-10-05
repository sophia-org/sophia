#![cfg(feature = "native-session")]

use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, ProtectionDevice, ProtectionDomainRole,
    ProtectionDomainSpec, ProtectionPath, SupervisedProcessKind, SupervisorCommand,
    SupervisorEvent,
};
use std::fs;
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::path::Path;
use std::time::{Duration, Instant};

/// The device is /dev/null, not a GPU. This checks the real protection builder,
/// proof executable and same-process exec boundary without device access.
#[test]
fn protected_proof_exec_checks_exclusions_before_releasing_the_client() {
    let root = std::env::temp_dir().join(format!("gpu-proof-domain-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let client = root.join("client");
    let marker = root.join("executed");
    fs::write(
        &client,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&client, fs::Permissions::from_mode(0o700)).unwrap();
    for case in [
        "valid",
        "extra_drm",
        "missing_device",
        "input",
        "x11",
        "runtime",
        "display",
        "wrong_identity",
        "overflow",
        "retired_socket",
        "no_file_endpoint",
        "relative_file_endpoint",
    ] {
        let _ = fs::remove_file(&marker);
        let mut domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
            .unwrap()
            .path(ProtectionPath::read_write(&root))
            .unwrap();
        if case != "missing_device" {
            domain = domain
                .device(ProtectionDevice::required_at(
                    "/dev/null",
                    "/dev/dri/renderD3",
                ))
                .unwrap();
        }
        if case == "extra_drm" {
            domain = domain
                .device(ProtectionDevice::required_at(
                    "/dev/null",
                    "/dev/dri/renderD4",
                ))
                .unwrap();
        }
        if matches!(case, "input" | "x11" | "runtime") {
            let empty = root.join("empty");
            fs::create_dir_all(&empty).unwrap();
            domain = domain
                .path(ProtectionPath::read_only_at(
                    &empty,
                    match case {
                        "input" => "/dev/input",
                        "x11" => "/tmp/.X11-unix",
                        _ => "/run/user",
                    },
                ))
                .unwrap();
        }
        if case == "overflow" {
            let crowded = root.join("crowded");
            fs::create_dir(&crowded).unwrap();
            for id in 0..65 {
                fs::write(crowded.join(id.to_string()), []).unwrap();
            }
            domain = domain
                .path(ProtectionPath::read_only_at(
                    &crowded,
                    "/dev/proof_inventory",
                ))
                .unwrap();
        }
        // The fixture declares its stdio: nothing ambient from whatever launched
        // the test reaches the proof, whose own descriptor check would refuse a
        // socket there (REVIEW-CODEX-17).
        let domain = domain.inherited_fds([]).unwrap();
        let mut spec = ProcessLaunchSpec::new(env!("CARGO_BIN_EXE_sophia"))
            .arg("sophia-shell-gpu-proof-exec")
            .arg(format!("--client={}", client.display()))
            .arg("--client-arg=--first")
            .arg("--client-arg=two words")
            .arg("--client-arg=")
            .env("SOPHIA_SHELL_GPU_MODE", "direct")
            .env(
                "SOPHIA_GPU_PROOF_OBSERVATION_ID",
                "0123456789abcdef0123456789abcdef",
            )
            .env("SOPHIA_SHELL_GPU_GRANT_EPOCH", "1")
            .env("SOPHIA_SHELL_GPU_RENDER_NODE", "/dev/dri/renderD3")
            .env(
                "SOPHIA_SHELL_GPU_DEVICE_MAJOR",
                if case == "wrong_identity" { "2" } else { "1" },
            )
            .env("SOPHIA_SHELL_GPU_DEVICE_MINOR", "3")
            .process_group()
            .protection_domain(domain);
        if case == "display" {
            spec = spec.env("DISPLAY", ":77");
        }
        // The client's one endpoint is the 9P export; the path is never opened.
        match case {
            "no_file_endpoint" => {}
            "relative_file_endpoint" => spec = spec.env("SOPHIA_SHELL_9P_SOCKET", "shell.sock"),
            _ => spec = spec.env("SOPHIA_SHELL_9P_SOCKET", "/run/shell/files.sock"),
        }
        if case == "retired_socket" {
            spec = spec.env("SOPHIA_SHELL_SOCKET", "/run/shell/socket.sock");
        }
        let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::Shell, spec);
        supervisor
            .apply(SupervisorCommand::StartProcess {
                process: SupervisedProcessKind::Shell,
                delay: Duration::ZERO,
            })
            .unwrap();
        assert!(supervisor.protection_evidence().is_some());
        let deadline = Instant::now() + Duration::from_secs(5);
        while supervisor.poll().unwrap() != Some(SupervisorEvent::ProcessExited) {
            assert!(Instant::now() < deadline, "proof child deadline: {case}");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(marker.exists(), case == "valid", "exec boundary: {case}");
        if case == "valid" {
            // The client runs with exactly the proof's arguments, in order.
            assert_eq!(
                fs::read_to_string(&marker).unwrap(),
                "--first\ntwo words\n\n"
            );
        }
    }
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_sophia"))
        .arg("sophia-shell-gpu-proof-exec")
        .arg(format!("--client={}", client.display()))
        .env_clear()
        .env(
            "SOPHIA_GPU_PROOF_OBSERVATION_ID",
            "0123456789abcdef0123456789abcdef",
        )
        .env("SOPHIA_SHELL_GPU_MODE", "direct")
        .env("SOPHIA_SHELL_GPU_GRANT_EPOCH", "1")
        .env("SOPHIA_SHELL_GPU_RENDER_NODE", "/dev/dri/renderD3")
        .env("SOPHIA_SHELL_GPU_DEVICE_MAJOR", "1")
        .env("SOPHIA_SHELL_GPU_DEVICE_MINOR", "3")
        .status()
        .unwrap();
    assert!(
        !status.success(),
        "grant environment alone is not domain evidence"
    );
    assert!(!marker.exists());
    fs::remove_dir_all(root).unwrap();
}

/// An otherwise valid proof whose domain passes on exactly `inherited` of this
/// process's standard descriptors. Returns whether its client ran.
fn valid_proof_ran(root: &Path, inherited: &[i32]) -> bool {
    let client = root.join("client");
    let marker = root.join("executed");
    let _ = fs::remove_file(&marker);
    fs::write(
        &client,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&client, fs::Permissions::from_mode(0o700)).unwrap();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
        .unwrap()
        .path(ProtectionPath::read_write(root))
        .unwrap()
        .device(ProtectionDevice::required_at(
            "/dev/null",
            "/dev/dri/renderD3",
        ))
        .unwrap()
        .inherited_fds(inherited.iter().copied())
        .unwrap();
    let spec = ProcessLaunchSpec::new(env!("CARGO_BIN_EXE_sophia"))
        .arg("sophia-shell-gpu-proof-exec")
        .arg(format!("--client={}", client.display()))
        .arg("--client-arg=--first")
        .env("SOPHIA_SHELL_GPU_MODE", "direct")
        .env(
            "SOPHIA_GPU_PROOF_OBSERVATION_ID",
            "0123456789abcdef0123456789abcdef",
        )
        .env("SOPHIA_SHELL_GPU_GRANT_EPOCH", "1")
        .env("SOPHIA_SHELL_GPU_RENDER_NODE", "/dev/dri/renderD3")
        .env("SOPHIA_SHELL_GPU_DEVICE_MAJOR", "1")
        .env("SOPHIA_SHELL_GPU_DEVICE_MINOR", "3")
        .env("SOPHIA_SHELL_9P_SOCKET", "/run/shell/files.sock")
        .process_group()
        .protection_domain(domain);
    let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::Shell, spec);
    supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::Shell,
            delay: Duration::ZERO,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while supervisor.poll().unwrap() != Some(SupervisorEvent::ProcessExited) {
        assert!(Instant::now() < deadline, "proof child deadline");
        std::thread::sleep(Duration::from_millis(5));
    }
    marker.exists()
}

const SOCKET_STDIN_CHILD: &str = "GPU_PROOF_DOMAIN_SOCKET_STDIN_CHILD";

/// A launcher whose stdin is a socket (some test runners and terminals) must
/// not decide the fixture: with its stdio declared the proof runs, and a proof
/// that does inherit that socket is refused, exactly, before its client runs.
/// This test re-runs itself with a socket stdin, so this process's own
/// descriptors are never changed.
#[test]
fn a_launchers_socket_stdin_reaches_only_a_proof_that_inherits_it() {
    if std::env::var_os(SOCKET_STDIN_CHILD).is_some() {
        assert!(
            fs::metadata("/proc/self/fd/0")
                .unwrap()
                .file_type()
                .is_socket(),
            "the re-run's stdin is the socket under test"
        );
        let root = std::env::temp_dir().join(format!("gpu-proof-stdin-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        assert!(
            valid_proof_ran(&root, &[]),
            "declared stdio: the proof runs"
        );
        // Stderr stays inherited so the refusal reaches the parent's capture.
        assert!(
            !valid_proof_ran(&root, &[0, 2]),
            "an inherited socket stdin: the client never runs"
        );
        fs::remove_dir_all(root).unwrap();
        return;
    }
    let scratch = std::env::temp_dir().join(format!("gpu-proof-stdin-run-{}", std::process::id()));
    fs::create_dir(&scratch).unwrap();
    let (stdin, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "a_launchers_socket_stdin_reaches_only_a_proof_that_inherits_it",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SOCKET_STDIN_CHILD, "1")
        .stdin(std::process::Stdio::from(std::os::fd::OwnedFd::from(stdin)))
        .stdout(fs::File::create(scratch.join("stdout")).unwrap())
        .stderr(fs::File::create(scratch.join("stderr")).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("socket-stdin re-run deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stderr = fs::read_to_string(scratch.join("stderr")).unwrap();
    let stdout = fs::read_to_string(scratch.join("stdout")).unwrap();
    assert!(status.success(), "re-run failed: {stdout}\n{stderr}");
    assert!(
        stderr.contains("GPU proof inherited a socket or unrelated device"),
        "the inheriting proof is refused for the socket: {stderr}"
    );
    fs::remove_dir_all(scratch).unwrap();
}
