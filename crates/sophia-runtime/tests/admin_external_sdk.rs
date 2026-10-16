//! Independently built SDK peers against the production administrative file service.
#[path = "support/admin_files.rs"]
mod support;
use std::{
    path::PathBuf,
    process::{Child, Command},
    time::{Duration, Instant},
};
use support::{ControlOutcome, Fixture};
struct Peer(Child);
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires SOPHIA_ADMIN_SDK_PEER naming an absolute independently built peer"]
fn external_sdk_settlement_stale_refresh_and_admission_revocation() {
    let executable = PathBuf::from(std::env::var_os("SOPHIA_ADMIN_SDK_PEER").unwrap());
    assert!(executable.is_absolute() && executable.is_file());
    for revoke in [false, true] {
        let f = Fixture::new();
        let path = f.service.socket_path();
        let report = path.parent().unwrap().join("sdk-report");
        let mut child = Peer(
            Command::new(&executable)
                .arg(path)
                .arg(&report)
                .arg(if revoke { "revoke" } else { "settle" })
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut tickets = 0;
        let mut refreshed = false;
        let mut revoked = None;
        loop {
            assert!(Instant::now() < deadline, "SDK peer deadline");
            if let Some(ticket) = f.service.try_request() {
                tickets += 1;
                assert_eq!(ticket.command, support::action());
                assert_eq!(ticket.request, if tickets == 1 { 1 } else { 3 });
                assert_eq!(ticket.generation, if tickets == 1 { 7 } else { 8 });
                support::claim(&ticket);
                if revoke {
                    assert_eq!(tickets, 1);
                    f.publish(7, &[child.0.id()]);
                    assert!(ticket.dispatched());
                    revoked = Some(ticket);
                } else {
                    // Leave an accepted ticket pending briefly before settling.
                    // This is a deterministic owner fixture, not a latency measurement.
                    std::thread::sleep(Duration::from_millis(20));
                    ticket.finish(ControlOutcome::Committed);
                }
            }
            if !revoke
                && !refreshed
                && std::fs::read_to_string(&report).ok().as_deref() == Some("first\n")
            {
                f.publish(8, &[]);
                std::fs::write(&report, b"continue\n").unwrap();
                refreshed = true;
            }
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success(), "SDK peer failed: {status}");
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(std::fs::read_to_string(&report).unwrap(), "ok\n");
        assert_eq!(tickets, if revoke { 1 } else { 2 });
        if let Some(ticket) = revoked {
            assert!(ticket.dispatched());
            ticket.finish(ControlOutcome::Committed);
        }
        assert!(f.service.try_request().is_none());
    }
}
