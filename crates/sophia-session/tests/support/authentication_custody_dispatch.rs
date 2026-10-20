//! Real protected child, Factotum and RoleFileAuthentication. Scope wiring to
//! actual requests and parked reads is tested by the runtime reactor controls.
use super::*;
use sophia_9p::{Errno, Fid, ReadOutcome};
use sophia_runtime::ProtectionPath;
use sophia_runtime::authenticated_files::{FileAuthentication, FileAuthenticationFactory};
use sophia_session::launch_authentication::files::TransportBudget;
use std::os::unix::fs::DirBuilderExt;

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Joined {
    owner: Box<dyn FileAuthentication>,
    attempt: Attempt,
    supervisor: ProcessSupervisor,
    fixture: Fixture,
    _directory: Directory,
}
impl Joined {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = Directory(PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "custody-dispatch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory.0)
            .unwrap();
        let program = directory.0.join("peer");
        assert!(
            std::process::Command::new("/usr/bin/cc")
                .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2"])
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/support/custody_dispatch_peer.c"
                ))
                .arg("-o")
                .arg(&program)
                .status()
                .unwrap()
                .success()
        );
        let nonce_path = directory.0.join("nonce");
        let mut fixture = Fixture::new();
        let id = fixture.reserve();
        let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
            .unwrap()
            .path(ProtectionPath::read_only(&directory.0))
            .unwrap();
        let mut supervisor = ProcessSupervisor::new(
            SupervisedProcessKind::Shell,
            ProcessLaunchSpec::new(&program)
                .arg(&nonce_path)
                .protection_domain(domain),
        );
        supervisor
            .start_with_launch_channel(fixture.book.take_channel(id, Instant::now()).unwrap())
            .unwrap();
        fixture.activate(id, &mut supervisor);
        let worker = fixture
            .book
            .worker_authority(id, &mut supervisor, Instant::now())
            .unwrap();
        let budget = TransportBudget::new(fixture.namespaces.clone());
        let endpoint = budget.endpoint();
        let mut factory = endpoint
            .bind(worker, b"role".to_vec(), b"view".to_vec())
            .unwrap();
        let mut owner = factory
            .create(ConnectionId(1), Instant::now(), fixture.wake.clone())
            .unwrap();
        // An unadmitted owner never receives a dispatch, even when called
        // through the experimental service entry during authentication.
        owner.begin_custody_dispatch().unwrap();
        let attempt = owner.begin(Fid(7), Instant::now()).unwrap();
        let end = deadline();
        let bytes = loop {
            fixture.tick(end);
            match owner.read(attempt, 0, 40, Instant::now()) {
                Ok(ReadOutcome::Ready(bytes)) => break bytes,
                Ok(ReadOutcome::Pending) | Err(Errno::EAGAIN) => {}
                other => panic!("challenge: {other:?}"),
            }
        };
        let nonce = sophia_factotum::launch::relay::decode_challenge(&bytes).unwrap();
        let hex = nonce
            .0
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        std::fs::write(&nonce_path, hex).unwrap();
        loop {
            fixture.tick(end);
            match owner.read(attempt, 40, 8, Instant::now()) {
                Ok(ReadOutcome::Ready(bytes)) if bytes == [1, 0, 0, 0, 0, 0, 0, 0] => break,
                Ok(_) | Err(Errno::EAGAIN) => {}
                other => panic!("verification: {other:?}"),
            }
        }
        owner.commit(attempt, Instant::now()).unwrap();
        owner.end_custody_dispatch();
        owner.check(ConnectionId(1), Instant::now()).unwrap();
        Self {
            owner,
            attempt,
            supervisor,
            fixture,
            _directory: directory,
        }
    }
    fn kill_peer_without_reap(&self) -> Resume {
        let wrapper =
            rustix::process::Pid::from_raw(self.supervisor.child_id().unwrap() as i32).unwrap();
        let peer =
            rustix::process::Pid::from_raw(self.supervisor.peer_id().unwrap() as i32).unwrap();
        assert_ne!(wrapper, peer);
        rustix::process::kill_process(wrapper, rustix::process::Signal::STOP).unwrap();
        let resume = Resume(wrapper);
        let mut fds = Vec::new();
        self.owner.visit_wait_fds(&mut |fd| fds.push(fd));
        assert_eq!(fds.len(), 2);
        rustix::process::kill_process(peer, rustix::process::Signal::KILL).unwrap();
        assert!(
            rustix::event::poll(
                &mut fds,
                Some(&rustix::event::Timespec {
                    tv_sec: 2,
                    tv_nsec: 0,
                })
            )
            .unwrap()
                > 0
        );
        assert!(fds[0].revents().is_empty());
        assert!(!fds[1].revents().is_empty());
        resume
    }
}
impl Drop for Joined {
    fn drop(&mut self) {
        self.owner.end_custody_dispatch();
        self.owner.close(Instant::now());
        let end = deadline();
        while !self.owner.drained() && Instant::now() < end {
            self.fixture.tick(end);
            let _ = self.owner.poll(Instant::now());
        }
        self.supervisor.terminate().unwrap();
    }
}
struct Resume(rustix::process::Pid);
impl Drop for Resume {
    fn drop(&mut self) {
        let _ = rustix::process::kill_process(self.0, rustix::process::Signal::CONT);
    }
}
#[test]
fn admitted_dispatch_preserves_permission_and_expiry_falls_back_to_fresh_custody() {
    let mut joined = Joined::new();
    joined.owner.begin_custody_dispatch().unwrap();
    for _ in 0..80 {
        joined.owner.poll(Instant::now()).unwrap();
        joined.owner.check(ConnectionId(1), Instant::now()).unwrap();
    }
    assert_eq!(
        joined.owner.check(ConnectionId(2), Instant::now()),
        Err(Errno::EACCES)
    );
    joined.owner.end_custody_dispatch();
    joined.owner.begin_custody_dispatch().unwrap();
    // Deliberately exceed the real 1ms bound. No timing-sensitive assertion
    // about how fast kill/wait completes or what an unexpired token observes.
    std::thread::sleep(Duration::from_millis(3));
    let resume = joined.kill_peer_without_reap();
    assert_eq!(
        joined.owner.check(ConnectionId(1), Instant::now()),
        Err(Errno::ESTALE)
    );
    // Admission must remain fresh even inside an experimental dispatch.
    assert!(joined.owner.commit(joined.attempt, Instant::now()).is_err());
    assert_eq!(joined.owner.poll(Instant::now()), Err(Errno::ESTALE));
    drop(resume);
}
#[test]
fn ending_dispatch_restores_fresh_checks_and_next_boundary_applies_peer_exit() {
    for new_boundary in [false, true] {
        let mut joined = Joined::new();
        joined.owner.begin_custody_dispatch().unwrap();
        joined.owner.end_custody_dispatch();
        let resume = joined.kill_peer_without_reap();
        if new_boundary {
            joined.owner.begin_custody_dispatch().unwrap();
        }
        assert_eq!(
            joined.owner.check(ConnectionId(1), Instant::now()),
            Err(Errno::ESTALE)
        );
        drop(resume);
    }
}
#[test]
fn launch_withdrawal_is_compared_inside_every_dispatch() {
    let mut joined = Joined::new();
    joined.owner.begin_custody_dispatch().unwrap();
    joined.supervisor.request_termination().unwrap();
    assert_eq!(
        joined.owner.check(ConnectionId(1), Instant::now()),
        Err(Errno::ESTALE)
    );
}
#[test]
fn namespace_withdrawal_is_compared_inside_every_dispatch() {
    let mut joined = Joined::new();
    joined.owner.begin_custody_dispatch().unwrap();
    joined
        .fixture
        .namespaces
        .lock()
        .unwrap()
        .revoke_namespace(sophia_protocol::NamespaceId::from_raw(9))
        .unwrap();
    assert_eq!(
        joined.owner.check(ConnectionId(1), Instant::now()),
        Err(Errno::ESTALE)
    );
}

#[test]
#[ignore = "run under strace to discriminate real RoleFileAuthentication dispatch wiring"]
fn count_real_role_owner_custody_probes() {
    use std::io::Write;
    let mut joined = Joined::new();
    for scoped in [false, true] {
        std::io::stderr()
            .write_all(if scoped {
                b"C40_BEGIN_SCOPED\n"
            } else {
                b"C40_BEGIN_FRESH\n"
            })
            .unwrap();
        for _ in 0..128 {
            if scoped {
                joined.owner.begin_custody_dispatch().unwrap();
            }
            for _ in 0..8 {
                joined.owner.poll(Instant::now()).unwrap();
                joined.owner.check(ConnectionId(1), Instant::now()).unwrap();
            }
            if scoped {
                joined.owner.end_custody_dispatch();
            }
            // The real shell service retains this final caller-fresh check.
            joined.owner.poll(Instant::now()).unwrap();
        }
    }
    std::io::stderr().write_all(b"C40_END_PROBES\n").unwrap();
}
