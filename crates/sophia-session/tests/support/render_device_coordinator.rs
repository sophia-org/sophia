use super::*;
use sophia_x_authority::{
    XServerFrontendDmaBufImportFormat, XServerFrontendRenderDeviceError,
    XServerFrontendRenderDeviceProvider,
};
use std::{
    fs::File,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Provider;
impl XServerFrontendRenderDeviceProvider for Provider {
    fn open_render_device_fd(
        &self,
    ) -> Result<std::os::fd::OwnedFd, XServerFrontendRenderDeviceError> {
        Ok(File::open("/dev/null").unwrap().into())
    }
    fn dma_buf_import_formats(&self) -> Vec<XServerFrontendDmaBufImportFormat> {
        Vec::new()
    }
}
fn bundle(generation: u64) -> Arc<Bundle> {
    Arc::new(Bundle::new(generation, Arc::new(Provider), None).unwrap())
}
fn identity(id: u64) -> Identity {
    Identity {
        node: format!("/dev/dri/renderD{id}").into(),
        device: 1,
        inode: id,
        device_number: id,
        physical_device: PathBuf::from(format!("/test/{id}")),
    }
}
fn device(identity: Identity) -> LiveRenderDevice {
    LiveRenderDevice {
        identity,
        file: File::open("/dev/null").unwrap(),
    }
}
fn prepared(request: PreparationRequest) -> Result<PreparedInventory, String> {
    let replacement = if request.replace_default {
        Ok(Some(PreparedBundle {
            bundle: bundle(request.bundle_generation),
            identity: request
                .identities
                .first()
                .ok_or("empty test inventory")?
                .clone(),
        }))
    } else {
        Ok(None)
    };
    Ok(PreparedInventory {
        devices: request.identities.into_iter().map(device).collect(),
        replacement,
    })
}
fn coordinator(
    prepare: impl FnMut(PreparationRequest) -> Result<PreparedInventory, String> + Send + 'static,
) -> LiveRenderDeviceCoordinator {
    LiveRenderDeviceCoordinator::with_preparer(
        "seat-test".into(),
        bundle(1),
        identity(1),
        vec![device(identity(1))],
        prepare,
    )
    .unwrap()
}
fn acknowledge(commands: &Receiver<Command>, installed: &mut Vec<u64>, lost: &mut Vec<u64>) {
    while let Ok(command) = commands.try_recv() {
        match command {
            Command::InstallDeviceBundle {
                bundle,
                acknowledgement,
            } => {
                installed.push(bundle.generation());
                acknowledgement.send(Ok(())).unwrap();
            }
            Command::MarkDeviceGenerationUnavailable {
                generation,
                acknowledgement,
            } => {
                lost.push(generation);
                acknowledgement.send(Ok(())).unwrap();
            }
            _ => panic!("unexpected frontend command"),
        }
    }
}
fn drive_until(
    coordinator: &mut LiveRenderDeviceCoordinator,
    now: Instant,
    frontend: &SyncSender<Command>,
    commands: &Receiver<Command>,
    done: impl Fn(&LiveRenderDeviceCoordinator) -> bool,
    installed: &mut Vec<u64>,
    lost: &mut Vec<u64>,
) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        coordinator.poll(now, frontend).unwrap();
        acknowledge(commands, installed, lost);
        if done(coordinator) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "coordinator made no bounded progress"
        );
        std::thread::yield_now();
    }
}

#[test]
fn adding_a_device_refreshes_inventory_without_replacing_a_healthy_default() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut coordinator = coordinator(move |request| {
        assert!(!request.replace_default);
        observed.fetch_add(1, Ordering::SeqCst);
        prepared(request)
    });
    let now = Instant::now();
    let (frontend, commands) = mpsc::sync_channel(4);
    coordinator
        .observe_inventory(&[identity(1), identity(2)], now)
        .unwrap();
    let mut installed = Vec::new();
    let mut lost = Vec::new();
    drive_until(
        &mut coordinator,
        now,
        &frontend,
        &commands,
        |state| !state.dirty,
        &mut installed,
        &mut lost,
    );
    assert_eq!(coordinator.active_generation, 1);
    assert_eq!(coordinator.admitted_inventory().unwrap().0, 2);
    assert_eq!(coordinator.admitted_inventory().unwrap().1.len(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(installed.is_empty() && lost.is_empty());
}

#[test]
fn stale_preparation_never_installs_and_busy_observation_keeps_the_latest_work() {
    let (entered, started) = mpsc::sync_channel(1);
    let (resume, waiting) = mpsc::sync_channel(1);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut coordinator = coordinator(move |request| {
        if observed.fetch_add(1, Ordering::SeqCst) == 0 {
            entered.send(()).unwrap();
            waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        prepared(request)
    });
    let now = Instant::now();
    let (frontend, commands) = mpsc::sync_channel(4);
    coordinator.observe_inventory(&[identity(2)], now).unwrap();
    coordinator.poll(now, &frontend).unwrap();
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    coordinator.observe_inventory(&[identity(3)], now).unwrap();
    let mut installed = Vec::new();
    let mut lost = Vec::new();
    for _ in 0..3 {
        coordinator.poll(now, &frontend).unwrap();
        acknowledge(&commands, &mut installed, &mut lost);
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "one preparation remains in flight"
    );
    resume.send(()).unwrap();
    drive_until(
        &mut coordinator,
        now,
        &frontend,
        &commands,
        |state| !state.dirty,
        &mut installed,
        &mut lost,
    );
    assert_eq!(installed, [3]);
    assert_eq!(lost, [1]);
    assert_eq!(coordinator.active_identity, identity(3));
    assert_eq!(coordinator.admitted_inventory().unwrap().0, 3);
}

#[test]
fn queue_and_bundle_capacity_preserve_the_prepared_candidate_for_retry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut coordinator = coordinator(move |request| {
        observed.fetch_add(1, Ordering::SeqCst);
        prepared(request)
    });
    let now = Instant::now();
    let (frontend, commands) = mpsc::sync_channel(1);
    frontend.send(Command::StopAccepting).unwrap();
    coordinator.observe_inventory(&[identity(2)], now).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while coordinator.install.is_none() {
        coordinator.poll(now, &frontend).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(
        coordinator.dirty
            && coordinator
                .install
                .as_ref()
                .unwrap()
                .acknowledgement
                .is_none()
    );
    assert!(matches!(commands.try_recv(), Ok(Command::StopAccepting)));
    let mut installed = Vec::new();
    let mut lost = Vec::new();
    coordinator.poll(now + RETRY, &frontend).unwrap();
    acknowledge(&commands, &mut installed, &mut lost);
    let retry = now + RETRY * 2;
    coordinator.poll(retry, &frontend).unwrap();
    let generation = match commands.recv_timeout(Duration::from_secs(2)).unwrap() {
        Command::InstallDeviceBundle {
            bundle,
            acknowledgement,
        } => {
            acknowledgement.send(Err(BundleError::Capacity)).unwrap();
            bundle.generation()
        }
        _ => panic!("expected install"),
    };
    coordinator.poll(retry, &frontend).unwrap();
    assert!(coordinator.dirty && commands.try_recv().is_err());
    drive_until(
        &mut coordinator,
        retry + RETRY,
        &frontend,
        &commands,
        |state| !state.dirty,
        &mut installed,
        &mut lost,
    );
    assert_eq!(installed, [generation]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn an_install_acknowledgement_after_device_loss_is_accounted_and_revoked() {
    let mut coordinator = coordinator(prepared);
    let now = Instant::now();
    let (frontend, commands) = mpsc::sync_channel(4);
    coordinator.observe_inventory(&[identity(2)], now).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let acknowledgement = loop {
        coordinator.poll(now, &frontend).unwrap();
        match commands.try_recv() {
            Ok(Command::MarkDeviceGenerationUnavailable {
                acknowledgement, ..
            }) => acknowledgement.send(Ok(())).unwrap(),
            Ok(Command::InstallDeviceBundle {
                bundle,
                acknowledgement,
            }) => {
                assert_eq!(bundle.generation(), 2);
                break acknowledgement;
            }
            Err(TryRecvError::Empty) => std::thread::yield_now(),
            _ => panic!("unexpected command"),
        }
        assert!(Instant::now() < deadline);
    };
    coordinator.observe_inventory(&[identity(3)], now).unwrap();
    acknowledgement.send(Ok(())).unwrap();
    coordinator.poll(now, &frontend).unwrap();
    assert_eq!(coordinator.active_generation, 2);
    assert!(!coordinator.active_available);
    assert!(coordinator.losses.contains_key(&2));
    coordinator.poll(now, &frontend).unwrap();
    match commands.recv_timeout(Duration::from_secs(2)).unwrap() {
        Command::MarkDeviceGenerationUnavailable {
            generation: 2,
            acknowledgement,
        } => {
            let _ = acknowledgement.send(Ok(()));
        }
        _ => panic!("installed but overtaken generation must be revoked"),
    }
}

#[test]
fn a_preparation_deadline_quarantines_one_worker_and_revalidates_its_late_result() {
    let (entered, started) = mpsc::sync_channel(1);
    let (resume, waiting) = mpsc::sync_channel(1);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut coordinator = coordinator(move |request| {
        if observed.fetch_add(1, Ordering::SeqCst) == 0 {
            entered.send(()).unwrap();
            waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        prepared(request)
    });
    let now = Instant::now();
    let (frontend, commands) = mpsc::sync_channel(4);
    coordinator.observe_inventory(&[identity(2)], now).unwrap();
    coordinator.poll(now, &frontend).unwrap();
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut installed = Vec::new();
    let mut lost = Vec::new();
    acknowledge(&commands, &mut installed, &mut lost);
    coordinator.poll(now, &frontend).unwrap();
    assert!(!coordinator.preparation.as_ref().unwrap().deadline_reported);
    let expired = now + ACK_TIMEOUT;
    for time in [expired, expired + RETRY, expired + RETRY * 2] {
        coordinator.poll(time, &frontend).unwrap();
        let flight = coordinator.preparation.as_ref().unwrap();
        assert_eq!(flight.ticket, 2);
        assert_eq!(flight.deadline, expired, "deadline never resets");
        assert!(flight.deadline_reported);
        assert!(coordinator.dirty && coordinator.install.is_none());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    coordinator
        .observe_inventory(&[identity(3)], expired)
        .unwrap();
    resume.send(()).unwrap();
    drive_until(
        &mut coordinator,
        expired + RETRY * 2,
        &frontend,
        &commands,
        |state| !state.dirty,
        &mut installed,
        &mut lost,
    );
    assert_eq!(
        installed,
        [3],
        "late stale preparation must not be installed"
    );
    assert_eq!(coordinator.active_identity, identity(3));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn failed_measurement_preserves_inventory_and_retries_without_advertising_it() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut coordinator = coordinator(move |request| {
        let failed = observed.fetch_add(1, Ordering::SeqCst) == 0;
        let mut result = prepared(request)?;
        if failed {
            result.replacement = Err("test capability query failed".into());
        }
        Ok(result)
    });
    let now = Instant::now();
    let (frontend, commands) = mpsc::sync_channel(4);
    coordinator.observe_inventory(&[identity(2)], now).unwrap();
    let mut installed = Vec::new();
    let mut lost = Vec::new();
    drive_until(
        &mut coordinator,
        now,
        &frontend,
        &commands,
        |state| state.admitted_sequence == 2,
        &mut installed,
        &mut lost,
    );
    assert!(coordinator.dirty && !coordinator.active_available);
    assert_eq!(coordinator.active_generation, 1);
    assert!(installed.is_empty());
    assert_eq!(coordinator.admitted[0].identity, identity(2));
    coordinator.poll(now, &frontend).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1, "retry is paced");
    drive_until(
        &mut coordinator,
        now + RETRY,
        &frontend,
        &commands,
        |state| !state.dirty,
        &mut installed,
        &mut lost,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(installed, [3]);
    assert!(coordinator.active_available);
}

#[test]
fn failed_preferred_device_does_not_hide_a_later_compatible_device() {
    let identities = vec![identity(1), identity(2), identity(3)];
    let devices = identities.iter().cloned().map(device).collect::<Vec<_>>();
    let request = PreparationRequest {
        ticket: 2,
        bundle_generation: 2,
        seat: "seat-test".into(),
        identities,
        preferred_physical: identity(2).physical_device,
        pixmap_textures: false,
        replace_default: true,
    };
    let mut attempted = Vec::new();
    let selected = worker::prepare_replacement(&request, &devices, |device| {
        attempted.push(device.identity.inode);
        if device.identity == identity(2) {
            Err("preferred device cannot preserve the capability contract".into())
        } else {
            Ok(bundle(2))
        }
    })
    .unwrap()
    .unwrap();
    assert_eq!(attempted, [2, 1], "stop at the first validated replacement");
    assert_eq!(selected.identity, identity(1));
    attempted.clear();
    assert!(
        worker::prepare_replacement(&request, &devices, |device| {
            attempted.push(device.identity.inode);
            Err("unavailable".into())
        })
        .is_err()
    );
    assert_eq!(attempted, [2, 1, 3], "every admitted device is tried once");
}
