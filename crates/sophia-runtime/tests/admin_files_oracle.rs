//! Third-party Go client judges the public records; Rust supplies owner timing.
#[allow(dead_code)]
#[path = "support/shell_files_oracle/process.rs"]
mod process;
#[path = "support/admin_files.rs"]
mod support;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Stdio};
use support::*;

const TRANSCRIPT: &str = "check discovery ok\ncheck modes ok\ncheck malformed ok\ncheck commit-and-retention ok\ncheck replay ok\ncheck stale-and-unadvertised ok\ncheck revocation ok\ncheck denied-reattach ok\nsophia_admin_oracle schema=1 status=pass checks=8\n";

fn run(binary: &Path, root: &Path, faulty: bool) {
    let f = Fixture::new();
    let control = root.join("control.sock");
    let listener = UnixListener::bind(&control).unwrap();
    listener.set_nonblocking(true).unwrap();
    let stdout = root.join("stdout");
    let stderr = root.join("stderr");
    let mut child = process::ChildGuard(
        Command::new(binary)
            .arg("-socket")
            .arg(f.service.socket_path())
            .arg("-control")
            .arg(&control)
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(&stdout).unwrap())
            .stderr(std::fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(25);
    let mut phases = 0;
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "oracle deadline");
        for path in [&stdout, &stderr] {
            assert!(
                std::fs::metadata(path).unwrap().len() < 8192,
                "oracle output bound"
            );
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                // A bounded read keeps an invalid peer from growing the fixture input.
                use std::io::Read;
                BufReader::new((&stream).take(64))
                    .read_line(&mut line)
                    .unwrap();
                match (phases, line.as_str()) {
                    (0, "finish\n") => {
                        let ticket = f.ticket();
                        assert_eq!(ticket.command, action());
                        assert_eq!((ticket.request, ticket.generation), (1, 7));
                        claim(&ticket);
                        ticket.finish(if faulty {
                            ControlOutcome::Rejected
                        } else {
                            ControlOutcome::Committed
                        });
                    }
                    (1, "advance\n") => {
                        assert!(f.service.try_request().is_none(), "replay reached owner");
                        f.publish(8, &[]);
                    }
                    (2, "revoke\n") => {
                        let ticket = f.ticket();
                        assert_eq!((ticket.request, ticket.generation), (4, 8));
                        assert_eq!(ticket.command, action());
                        f.publish(8, &[child.0.id()]);
                        until(|| ticket.cancelled());
                        assert!(!ticket.claim());
                    }
                    _ => panic!("unexpected phase {phases}: {line:?}"),
                }
                phases += 1;
                stream.write_all(b"ok\n").unwrap();
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("control accept: {e}"),
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let output = std::fs::read_to_string(stdout).unwrap();
    let errors = std::fs::read_to_string(stderr).unwrap();
    if faulty {
        assert!(!status.success(), "faulty owner passed\n{output}");
        assert!(
            errors.contains("settlement outcome"),
            "wrong negative: {errors}"
        );
        assert_eq!(phases, 1);
    } else {
        assert!(status.success(), "oracle failed\n{output}\n{errors}");
        assert_eq!(output, TRANSCRIPT);
        assert_eq!(phases, 3);
        assert!(f.service.try_request().is_none());
        println!("{output}");
    }
    std::fs::remove_file(control).unwrap();
}

#[test]
fn independent_client_checks_admin_records_and_rejects_wrong_settlement() {
    let root = std::env::temp_dir().join(format!("sophia-admin-oracle-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let scratch = process::Scratch(root);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = process::build_named(&repo, &scratch.0, "admin-oracle");
    run(&binary, &scratch.0, false);
    run(&binary, &scratch.0, true);
}
