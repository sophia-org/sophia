//! Opt-in installed provider check, without display or input devices.
use super::*;
use sophia_protocol::lock_files::{LockCandidateOutcome, LockCandidateStatus};
use sophia_runtime::lock_files::LockInbound;

#[test]
#[ignore = "requires KLEIS_AUTH_PEER installed executable"]
fn installed_kleis_survives_idle_and_submits_a_lock_image() {
    let executable = std::path::PathBuf::from(std::env::var_os("KLEIS_AUTH_PEER").unwrap());
    let directory = std::env::temp_dir().join(format!("kleis-installed-{}", std::process::id()));
    let limits = crate::session_lock_object::session_lock_file_limits(None);
    let mut transport = LockFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        1,
        limits,
    )
    .unwrap();
    transport
        .enable_isb_images(crate::session_lock_object::session_lock_image_policy(
            &limits,
        ))
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let base = ProcessLaunchSpec::new(executable)
        .env(sophia_runtime::SOPHIA_LOCK_9P_SOCKET_ENV, &socket)
        .arg("--no-gpu")
        .process_group()
        .protection_domain(
            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::LockProvider])
                .unwrap()
                .path(ProtectionPath::read_only(&directory))
                .unwrap(),
        );
    let mut auth = factotum::Fixture::new();
    let wake = sophia_wake::Wake::new().unwrap();
    let mut object = locked();
    object.phase = LockPhase::Unlocked;
    object.lock_epoch = 0;
    object.allocations.clear();
    let mut provider = LockProvider::with_service(
        transport,
        base,
        ShellGpuMode::Denied,
        None,
        object,
        Vec::new(),
        wake.notifier(),
        policy(),
        auth.coordinator.endpoint(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut connected = None;
    let mut locked_sent = false;
    let mut resource_ready = false;
    let mut candidates = 0;
    while Instant::now() < deadline {
        auth.tick();
        for event in provider.poll(Instant::now(), &mut auth.coordinator) {
            eprintln!("installed provider event: {event:?}");
            match event {
                LockFileServiceEvent::Connected {
                    connection_epoch, ..
                } => {
                    assert_eq!(connection_epoch, 1);
                    connected = Some(Instant::now());
                }
                LockFileServiceEvent::Disconnected { .. } => {
                    panic!("installed provider disconnected")
                }
                LockFileServiceEvent::Inbound {
                    inbound:
                        LockInbound::ResourceReady {
                            width_px,
                            height_px,
                            pixels,
                            ..
                        },
                    ..
                } => {
                    assert_eq!((width_px, height_px), (4, 4));
                    assert_eq!(pixels.len(), 64);
                    resource_ready = true;
                }
                LockFileServiceEvent::Inbound {
                    inbound: LockInbound::Demand(demand),
                    ..
                } => {
                    provider
                        .service
                        .command(LockFileServiceCommand::Permit {
                            allocation_id: demand.allocation_id,
                            demand_id: demand.demand_id,
                            expires_after: Duration::from_millis(250),
                        })
                        .unwrap();
                }
                LockFileServiceEvent::Inbound {
                    inbound: LockInbound::Candidate { candidate, .. },
                    ..
                } => {
                    assert!(locked_sent && resource_ready);
                    candidates += 1;
                    if candidates == 3 {
                        return;
                    }
                    provider
                        .service
                        .command(LockFileServiceCommand::Outcome(LockCandidateOutcome {
                            transaction: candidate.transaction,
                            lock_epoch: candidate.lock_epoch,
                            output_id: candidate.output_id,
                            allocation_id: candidate.allocation_id,
                            candidate_generation: candidate.candidate_generation,
                            status: LockCandidateStatus::Presented,
                            reason: 0,
                        }))
                        .unwrap();
                }
                _ => {}
            }
        }
        if !locked_sent && connected.is_some_and(|at| at.elapsed() >= Duration::from_secs(3)) {
            provider
                .service
                .command(LockFileServiceCommand::PublishLock(locked()))
                .unwrap();
            locked_sent = true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("installed provider failed to stay connected");
}
