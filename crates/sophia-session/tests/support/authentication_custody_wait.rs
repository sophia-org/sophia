use super::*;
use sophia_runtime::authenticated_files::{AuthenticatedFiles, FileAuthenticationFactory};
use sophia_session::launch_authentication::files::TransportBudget;

#[test]
fn custody_exit_wakes_authentication_and_closed_owner_unsubscribes() {
    let mut f = Fixture::new();
    let id = f.reserve();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell]).unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::Shell,
        ProcessLaunchSpec::new("/usr/bin/sleep")
            .arg("30")
            .protection_domain(domain),
    );
    supervisor
        .start_with_launch_channel(f.book.take_channel(id, Instant::now()).unwrap())
        .unwrap();
    f.activate(id, &mut supervisor);
    let worker = f
        .book
        .worker_authority(id, &mut supervisor, Instant::now())
        .unwrap();
    let budget = TransportBudget::new(f.namespaces.clone());
    let endpoint = budget.endpoint();
    let mut factory = endpoint
        .bind(worker, b"role".to_vec(), b"role".to_vec())
        .unwrap();
    let owner = factory
        .create(ConnectionId(1), Instant::now(), f.wake.clone())
        .unwrap();
    let mut files = AuthenticatedFiles::pending(());
    files.install(owner).unwrap();
    files.poll(Instant::now()).unwrap();
    {
        let mut fds = Vec::new();
        files.visit_authentication_fds(|fd| fds.push(fd));
        assert_eq!(fds.len(), 2);
        assert_eq!(
            rustix::event::poll(&mut fds, Some(&rustix::event::Timespec::default())).unwrap(),
            0
        );
        let peer = rustix::process::Pid::from_raw(supervisor.peer_id().unwrap() as i32).unwrap();
        rustix::process::kill_process(peer, rustix::process::Signal::KILL).unwrap();
        assert!(
            rustix::event::poll(
                &mut fds,
                Some(&rustix::event::Timespec {
                    tv_sec: 2,
                    tv_nsec: 0
                })
            )
            .unwrap()
                > 0
        );
    }
    assert_eq!(files.poll(Instant::now()), Err(sophia_9p::Errno::ESTALE));
    let mut count = 0;
    files.visit_authentication_fds(|_| count += 1);
    assert_eq!(count, 0);
    supervisor.terminate().unwrap();
}
