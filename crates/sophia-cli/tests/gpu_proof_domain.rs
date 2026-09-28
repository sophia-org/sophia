#![cfg(feature = "native-session")]

use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, ProtectionDevice, ProtectionDomainRole,
    ProtectionDomainSpec, ProtectionPath, SupervisedProcessKind, SupervisorCommand,
    SupervisorEvent,
};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
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
