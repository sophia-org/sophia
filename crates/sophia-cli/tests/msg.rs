use sophia_protocol::*;
use sophia_runtime::*;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn cli_discovers_invokes_and_reports_machine_outcomes() {
    check_cli(false);
    check_cli(true);
}

fn check_cli(files: bool) {
    let transport = if files { "9p2000.L" } else { "current-ipc" };
    let root = std::env::temp_dir().join(format!("sc-cli-{}-{files}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let service = if files {
        ControlService::bind_files(&root)
    } else {
        ControlService::bind(&root)
    }
    .unwrap();
    let catalog = Arc::new(ControlCatalog {
        generation: 4,
        commands: vec![ControlCommand {
            owner: ControlOwner::Policy,
            name: "-focus next".into(),
        }],
    });
    while !service.publish(catalog.clone(), &[]) {
        std::thread::yield_now();
    }
    let cli = env!("CARGO_BIN_EXE_sophia");
    let listed = Command::new(cli)
        .args(["msg", "--transport", transport])
        .args(["--json", "--socket"])
        .arg(service.socket_path())
        .arg("commands")
        .output()
        .unwrap();
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    assert!(String::from_utf8_lossy(&listed.stdout).contains("\"catalog_generation\":4"));
    if files {
        let default = Command::new(cli)
            .args(["msg", "--json", "--socket"])
            .arg(service.socket_path())
            .arg("commands")
            .output()
            .unwrap();
        assert!(
            default.status.success(),
            "{}",
            String::from_utf8_lossy(&default.stderr)
        );
        assert_eq!(default.stdout, listed.stdout);
    }
    for (outcome, exit) in [
        (ControlOutcome::Committed, 0),
        (ControlOutcome::Rejected, 1),
        (ControlOutcome::Indeterminate, 1),
    ] {
        let child = Command::new(cli)
            .args(["msg", "--transport", transport, "--json", "--socket"])
            .arg(service.socket_path())
            .args(["policy", "--", "-focus next"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let ticket = loop {
            if let Some(ticket) = service.try_request() {
                break ticket;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        while !ticket.claim() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        ticket.finish(outcome);
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(exit));
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains(&format!("\"outcome\":\"{}\"", outcome.name()))
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains(if files {
            "\"request_id\":1"
        } else {
            "\"request_id\":2"
        }));
    }
    let absent = Command::new(cli)
        .args(["msg", "--transport", transport, "--json", "--socket"])
        .arg(service.socket_path())
        .args(["session", "reload-profile"])
        .output()
        .unwrap();
    assert_eq!(absent.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&absent.stderr).contains("not-invoked"));
    assert!(service.try_request().is_none());
    drop(service);
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn cli_requires_explicit_discovery_and_valid_syntax() {
    for args in [
        vec!["msg", "--json", "commands"],
        vec!["msg", "--json", "--socket", "relative", "commands"],
        vec!["msg", "--json", "policy"],
        vec!["msg", "--json", "--transport", "invalid", "commands"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sophia"))
            .args(args)
            .env_remove(SOPHIA_CONTROL_SOCKET_ENV)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}
