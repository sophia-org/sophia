//! CLI against the real 9P export, with supplied owner outcomes. Actual process
//! custody is qualified by Session's developer_recipe_owner joined test.
use sophia_protocol::developer_launch::*;
use sophia_runtime::developer_launch::DeveloperLaunchService;
use std::os::unix::fs::PermissionsExt;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn launch_cli_preserves_arguments_and_reports_correlated_owner_outcomes() {
    let root = std::env::temp_dir().join(format!("dl-cli-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let service = DeveloperLaunchService::bind(&root).unwrap();
    for status in [
        DeveloperLaunchStatus::Started,
        DeveloperLaunchStatus::Denied,
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sophia"))
            .args(["launch", "--socket"])
            .arg(service.socket_path())
            .args([
                "--recipe",
                "tools",
                "--",
                "/usr/bin/example",
                "a b",
                "",
                "$(not-a-shell)",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let ticket = loop {
            if let Some(ticket) = service.try_request() {
                break ticket;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("CLI submit deadline");
            }
            assert!(
                child.try_wait().unwrap().is_none(),
                "CLI exited before submission"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(ticket.request.recipe, "tools");
        assert_eq!(ticket.request.executable, "/usr/bin/example");
        assert_eq!(ticket.request.arguments, ["a b", "", "$(not-a-shell)"]);
        while !ticket.claim() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let process = (status == DeveloperLaunchStatus::Started).then_some(123);
        assert!(ticket.finish(status, process));
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("CLI result deadline");
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.success(),
            status == DeveloperLaunchStatus::Started,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result = decode_developer_launch_result(&output.stdout).unwrap();
        assert_eq!(result.status, status);
        assert_eq!(result.request, ticket.request.request);
        assert_eq!(result.process, process);
    }
    drop(service);
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn launch_cli_refuses_ambiguous_or_unbounded_requests_before_connecting() {
    for args in [
        vec![],
        vec!["--recipe", "tools", "--", "/usr/bin/true"],
        vec![
            "--socket",
            "relative",
            "--recipe",
            "tools",
            "--",
            "/usr/bin/true",
        ],
        vec![
            "--socket",
            "/unused",
            "--recipe",
            "tools",
            "--recipe",
            "admin",
            "--",
            "/usr/bin/true",
        ],
        vec![
            "--socket",
            "/unused",
            "--recipe",
            "../admin",
            "--",
            "/usr/bin/true",
        ],
        vec!["--socket", "/unused", "--recipe", "tools", "--", "relative"],
        vec![
            "--socket",
            "/unused",
            "--role",
            "admin",
            "--",
            "/usr/bin/true",
        ],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_sophia"))
            .arg("launch")
            .args(args)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(
            !String::from_utf8_lossy(&result.stderr).contains("9P socket"),
            "must refuse before connecting"
        );
    }
    let help = Command::new(env!("CARGO_BIN_EXE_sophia"))
        .args(["launch", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("host-admin"));
}
