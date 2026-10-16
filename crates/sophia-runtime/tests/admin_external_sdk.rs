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

struct Reports(PathBuf);
impl Drop for Reports {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires SOPHIA_ADMIN_SDK_PEER naming an absolute independently built peer"]
fn external_sdk_explicit_recovery_does_not_replay_or_inherit_old_completion() {
    let executable = PathBuf::from(std::env::var_os("SOPHIA_ADMIN_SDK_PEER").unwrap());
    assert!(executable.is_absolute() && executable.is_file());
    let reports =
        Reports(std::env::temp_dir().join(format!("admin-sdk-recovery-{}", std::process::id())));
    std::fs::create_dir(&reports.0).unwrap();
    let report = reports.0.join("result");
    let old = Fixture::new();
    let mut child = Peer(
        Command::new(executable)
            .arg(old.service.socket_path())
            .arg(&report)
            .arg("recover")
            .spawn()
            .unwrap(),
    );
    let prior = old.ticket();
    assert_eq!(prior.command, support::action());
    support::claim(&prior);
    drop(old);
    support::until(|| std::fs::read_to_string(&report).ok().as_deref() == Some("lost\n"));
    let fresh = Fixture::new();
    let probe = support::ControlCommand {
        owner: support::ControlOwner::Policy,
        name: "probe".into(),
    };
    let catalog = std::sync::Arc::new(support::ControlCatalog {
        generation: 9,
        commands: vec![probe.clone()],
    });
    support::until(|| fresh.service.publish(catalog.clone(), &[]));
    std::fs::write(
        &report,
        format!("recover {}\n", fresh.service.socket_path().display()),
    )
    .unwrap();
    let successor = fresh.ticket();
    assert_eq!(successor.command, probe);
    assert_eq!(
        (prior.connection, prior.request),
        (successor.connection, successor.request)
    );
    support::claim(&successor);
    prior.finish(ControlOutcome::Committed);
    // The new request deliberately has matching numeric identities. The old
    // owner's late result must neither settle it nor cause a replay.
    std::thread::sleep(Duration::from_millis(20));
    assert!(child.0.try_wait().unwrap().is_none());
    assert!(successor.dispatched());
    successor.finish(ControlOutcome::Rejected);
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        assert!(Instant::now() < deadline, "SDK recovery deadline");
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "SDK recovery peer failed: {status}");
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(std::fs::read_to_string(&report).unwrap(), "ok\n");
    assert!(fresh.service.try_request().is_none());
}

#[test]
#[ignore = "requires SOPHIA_ADMIN_SDK_PEER naming an absolute independently built peer"]
fn external_sdk_service_shutdown_preserves_uncertainty_and_owner_custody() {
    let executable = PathBuf::from(std::env::var_os("SOPHIA_ADMIN_SDK_PEER").unwrap());
    assert!(executable.is_absolute() && executable.is_file());
    for claimed in [false, true] {
        let reports = Reports(
            std::env::temp_dir().join(format!("admin-sdk-loss-{}-{claimed}", std::process::id())),
        );
        std::fs::create_dir(&reports.0).unwrap();
        let report = reports.0.join("result");
        let f = Fixture::new();
        let path = f.service.socket_path().to_owned();
        let mut child = Peer(
            Command::new(&executable)
                .arg(&path)
                .arg(&report)
                .arg("loss")
                .spawn()
                .unwrap(),
        );
        let ticket = f.ticket();
        assert_eq!(ticket.request, 1);
        assert_eq!(ticket.command, support::action());
        if claimed {
            support::claim(&ticket);
        }
        // Stop and join the real worker, closing its sockets. This is service
        // teardown, not a claim that the worker process was killed abruptly.
        drop(f);
        assert!(!path.exists());
        if claimed {
            assert!(ticket.dispatched());
        } else {
            assert!(ticket.cancelled());
            assert!(!ticket.claim());
        }
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            assert!(Instant::now() < deadline, "SDK shutdown deadline");
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success(), "SDK shutdown peer failed: {status}");
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(std::fs::read_to_string(&report).unwrap(), "ok\n");
        if claimed {
            ticket.finish(ControlOutcome::Committed);
        }
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
