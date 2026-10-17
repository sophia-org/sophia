//! Independent SDK executable over a privately admitted production export.
use super::*;
use sophia_9p::unix::Server;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires SOPHIA_REQUESTER_SDK_PEER naming an independently built executable"]
fn external_sdk_requester_production_lifecycle() {
    let executable =
        std::path::PathBuf::from(std::env::var_os("SOPHIA_REQUESTER_SDK_PEER").expect("SDK peer"));
    assert!(executable.is_absolute() && executable.is_file());
    for mode in [
        "completed",
        "revoked",
        "denied",
        "cancel",
        "disconnect",
        "lost-outcome",
        "cancel-partial",
    ] {
        for length in [0usize, 65536] {
            if mode == "cancel-partial" && length == 0 {
                continue;
            }
            let export = PortalFileExport::new(
                Authority {
                    revision: 1,
                    rights: true,
                    allow: mode != "denied",
                    live: true,
                    now: 100,
                    generation: 9,
                },
                PortalFileLimits::defaults(1),
            )
            .unwrap();
            let mut server = Server::new(export, sophia_9p::Limits::default()).unwrap();
            let (owner, stream) = UnixStream::pair().unwrap();
            let disconnect = owner.try_clone().unwrap();
            let connection = server.adopt(owner).unwrap_or_else(|_| panic!("adopt"));
            let mut admitted = admission(1);
            admitted.connection = connection;
            admitted.custody = PortalRequesterCustody::SessionAuthority {
                registration: std::num::NonZeroU64::new(1).unwrap(),
            };
            server.export_mut().admit(admitted).unwrap();
            // Only this socket is admitted. Stdin is a transport, not a credential.
            let mut child = Command::new(&executable)
                .args([mode, &length.to_string()])
                .stdin(Stdio::from(OwnedFd::from(stream)))
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut executions = 0;
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("SDK peer timed out: {mode}/{length}");
                }
                server.export_mut().expire(Instant::now());
                server.turn(Some(Duration::from_millis(1))).unwrap();
                if let Some(work) = server.export_mut().take_execution() {
                    executions += 1;
                    assert!(matches!(mode, "completed" | "revoked" | "lost-outcome"));
                    assert_eq!(work.payload, vec![17; length]);
                    if mode == "lost-outcome" {
                        disconnect.shutdown(std::net::Shutdown::Both).unwrap();
                    }
                    server
                        .export_mut()
                        .settle_execution(work.grant.transfer, mode != "revoked")
                        .unwrap();
                }
            };
            assert!(status.success(), "SDK peer failed: {mode}/{length}");
            while server.connection_count() != 0 {
                assert!(Instant::now() < deadline, "transport did not retire");
                server.turn(Some(Duration::from_millis(1))).unwrap();
                assert!(
                    server.export_mut().take_execution().is_none(),
                    "execution appeared after peer exit"
                );
            }
            assert!(server.export_mut().take_execution().is_none());
            assert_eq!(
                executions,
                usize::from(matches!(mode, "completed" | "revoked" | "lost-outcome"))
            );
        }
    }
}
