// Included by the existing worker-private test module.

use crate::LiveRenderedScanoutBufferExporter;
use std::time::Instant;

#[test]
fn a_detach_cannot_remove_a_replacement_or_another_output() {
    let (commands, receive) = sync_channel(32);
    let output = LiveRendererWorkerOutputKey::from_raw(401);
    let neighbour = LiveRendererWorkerOutputKey::from_raw(402);
    let detached = std::sync::Arc::new(super::OutputClaim::default());
    let old_results = register_claim(&commands, output, std::sync::Arc::clone(&detached));
    detached.detach();
    let new_results = register(&commands, output);
    let neighbour_results = register(&commands, neighbour);
    commands
        .send(WorkerCommand::Deregister {
            output,
            claim: detached,
        })
        .unwrap();
    render(&commands, output, 1);
    render(&commands, neighbour, 2);
    let worker = std::thread::spawn(move || {
        run_worker::<std::fs::File>(
            Err(std::io::Error::other("no render device in this test")),
            Vec::new(),
            receive,
            std::sync::Arc::new(super::WorkerControl::default()),
        );
    });
    assert_eq!(
        new_results.recv_timeout(SETTLE).unwrap().request_id,
        LiveRendererWorkerRequestId(1)
    );
    assert_eq!(
        neighbour_results.recv_timeout(SETTLE).unwrap().request_id,
        LiveRendererWorkerRequestId(2)
    );
    assert!(old_results.try_recv().is_err());
    drop(commands);
    worker.join().unwrap();
}

#[test]
fn a_full_queue_keeps_detach_claims_without_processing_their_renders() {
    let (commands, receive) = sync_channel(2);
    let output = LiveRendererWorkerOutputKey::from_raw(403);
    let claim = std::sync::Arc::new(super::OutputClaim::default());
    let results = register_claim(&commands, output, std::sync::Arc::clone(&claim));
    render(&commands, output, 1);
    claim.detach();
    assert!(matches!(
        commands.try_send(WorkerCommand::Deregister { output, claim }),
        Err(std::sync::mpsc::TrySendError::Full(_))
    ));
    drop(commands);
    run_worker::<std::fs::File>(
        Err(std::io::Error::other("no render device in this test")),
        Vec::new(),
        receive,
        std::sync::Arc::new(super::WorkerControl::default()),
    );
    assert!(
        results.try_recv().is_err(),
        "a detached registration must not render"
    );
}

#[test]
fn shutdown_wins_over_queued_commands_when_its_wakeup_is_full() {
    let (commands, receive) = sync_channel(2);
    let output = LiveRendererWorkerOutputKey::from_raw(404);
    let results = register(&commands, output);
    render(&commands, output, 1);
    let control = std::sync::Arc::new(super::WorkerControl::default());
    control.shutdown();
    assert!(matches!(
        commands.try_send(WorkerCommand::Shutdown),
        Err(std::sync::mpsc::TrySendError::Full(_))
    ));
    run_worker::<std::fs::File>(
        Err(std::io::Error::other("no render device in this test")),
        Vec::new(),
        receive,
        control,
    );
    assert!(
        results.try_recv().is_err(),
        "shutdown must precede queued rendering"
    );
}

#[test]
fn dropping_a_full_queue_facade_and_core_does_not_wait_for_a_stalled_worker() {
    let registry = std::sync::Arc::new(super::WorkerRegistry::default());
    let identity = super::DeviceIdentity {
        device: 1,
        inode: 900,
        special_device: 226 << 8,
    };
    let control = std::sync::Arc::new(super::WorkerControl::default());
    let service_control = std::sync::Arc::clone(&control);
    let (commands, receive) = sync_channel(2);
    let (resume, gate) = sync_channel(1);
    let (finished, finished_receiver) = sync_channel(1);
    let thread = registry
        .reserve(identity)
        .unwrap()
        .start(|| {
            std::thread::Builder::new().spawn(move || {
                gate.recv_timeout(SETTLE * 4).unwrap();
                run_worker::<std::fs::File>(
                    Err(std::io::Error::other("no render device in this test")),
                    Vec::new(),
                    receive,
                    service_control,
                );
                finished.send(()).unwrap();
            })
        })
        .unwrap();
    let core = std::sync::Arc::new(super::NativeGbmRendererWorkerCore {
        command_sender: commands,
        _thread: std::sync::Mutex::new(thread),
        control,
        inventory_replacement: std::sync::Mutex::new(None),
        release_enqueue_failures: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    });
    let output = LiveRendererWorkerOutputKey::from_raw(405);
    let facade = core.attach(output);
    render(&core.command_sender, output, 1);
    let (closed, close_receiver) = sync_channel(1);
    let closer = std::thread::spawn(move || {
        drop(facade);
        drop(core);
        closed.send(()).unwrap();
    });
    let close = close_receiver.recv_timeout(SETTLE);
    let predecessor_refused = registry.reserve(identity).is_err();
    resume.send(()).unwrap();
    closer.join().unwrap();
    finished_receiver.recv_timeout(SETTLE).unwrap();
    assert!(
        close.is_ok(),
        "facade or core destruction waited for command service"
    );
    assert!(
        predecessor_refused,
        "destruction lost the unfinished worker reservation"
    );
}

fn inventory_test_core(
    capacity: usize,
) -> (
    std::sync::Arc<super::NativeGbmRendererWorkerCore>,
    Receiver<WorkerCommand>,
    SyncSender<()>,
) {
    let registry = std::sync::Arc::new(super::WorkerRegistry::default());
    let identity = super::DeviceIdentity {
        device: 1,
        inode: 901,
        special_device: 226 << 8,
    };
    let (commands, receive) = sync_channel(capacity);
    let (resume, gate) = sync_channel(1);
    let thread = registry
        .reserve(identity)
        .unwrap()
        .start(|| {
            std::thread::Builder::new().spawn(move || {
                let _ = gate.recv_timeout(SETTLE * 4);
            })
        })
        .unwrap();
    let core = std::sync::Arc::new(super::NativeGbmRendererWorkerCore {
        command_sender: commands,
        _thread: std::sync::Mutex::new(thread),
        control: std::sync::Arc::new(super::WorkerControl::default()),
        inventory_replacement: std::sync::Mutex::new(None),
        release_enqueue_failures: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    });
    (core, receive, resume)
}

#[test]
fn inventory_replacement_allows_one_pending_exact_generation() {
    let (core, receive, resume) = inventory_test_core(2);
    assert_eq!(core.poll_image_import_device_replacement().unwrap(), None);
    core.request_image_import_device_replacement(7, Vec::new())
        .unwrap();
    let WorkerCommand::ReplaceImageImportDevices {
        generation,
        devices,
        completion_sender,
    } = receive.recv().unwrap()
    else {
        panic!("replacement request was not queued");
    };
    assert_eq!(generation, 7);
    assert!(devices.is_empty());
    assert_eq!(
        core.request_image_import_device_replacement(8, Vec::new())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(core.poll_image_import_device_replacement().unwrap(), None);
    completion_sender.send(Ok(generation)).unwrap();
    assert_eq!(
        core.poll_image_import_device_replacement().unwrap(),
        Some(7)
    );
    assert_eq!(core.poll_image_import_device_replacement().unwrap(), None);
    core.request_image_import_device_replacement(8, Vec::new())
        .unwrap();
    let WorkerCommand::ReplaceImageImportDevices {
        generation,
        completion_sender,
        ..
    } = receive.recv().unwrap()
    else {
        panic!("replacement request was not queued");
    };
    assert_eq!(generation, 8);
    completion_sender
        .send(Err(std::io::Error::from(std::io::ErrorKind::WouldBlock)))
        .unwrap();
    assert_eq!(
        core.poll_image_import_device_replacement()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    core.request_image_import_device_replacement(8, Vec::new())
        .unwrap();
    let WorkerCommand::ReplaceImageImportDevices {
        completion_sender, ..
    } = receive.recv().unwrap()
    else {
        panic!("replacement retry was not queued");
    };
    drop(completion_sender);
    assert_eq!(
        core.poll_image_import_device_replacement()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert_eq!(core.poll_image_import_device_replacement().unwrap(), None);
    resume.send(()).unwrap();
}

#[test]
fn explicit_core_shutdown_survives_full_wakeup_and_reports_only_joined_completion() {
    let (core, receive, resume) = inventory_test_core(1);
    assert!(core.poll_shutdown().is_err());
    core.command_sender.try_send(WorkerCommand::Shutdown).unwrap();
    core.request_shutdown();
    assert!(core.control.is_shutdown());
    assert!(!core.poll_shutdown().unwrap());
    resume.send(()).unwrap();
    let deadline = Instant::now() + SETTLE;
    while !core.poll_shutdown().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(core.poll_shutdown().unwrap());
    assert!(matches!(receive.try_recv(), Ok(WorkerCommand::Shutdown)));
}

#[test]
fn inventory_queue_refusal_keeps_the_pending_slot_empty() {
    let (core, receive, resume) = inventory_test_core(1);
    core.command_sender.send(WorkerCommand::Shutdown).unwrap();
    assert_eq!(
        core.request_image_import_device_replacement(9, Vec::new())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(core.poll_image_import_device_replacement().unwrap(), None);
    assert!(matches!(receive.recv().unwrap(), WorkerCommand::Shutdown));
    core.request_image_import_device_replacement(9, Vec::new())
        .unwrap();
    let WorkerCommand::ReplaceImageImportDevices {
        completion_sender,
        generation,
        ..
    } = receive.recv().unwrap()
    else {
        panic!("replacement retry was not queued");
    };
    completion_sender.send(Ok(generation)).unwrap();
    assert_eq!(
        core.poll_image_import_device_replacement().unwrap(),
        Some(9)
    );
    resume.send(()).unwrap();
}

#[test]
fn inventory_busy_bridge_and_failure_never_acknowledge_applied() {
    assert_eq!(
        super::inventory_replacement_result(11, Ok(true)).unwrap(),
        11
    );
    assert_eq!(
        super::inventory_replacement_result(11, Ok(false))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        super::inventory_replacement_result(
            11,
            Err(super::LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable)
        )
        .unwrap_err()
        .kind(),
        std::io::ErrorKind::Other
    );
    let (commands, worker) = worker_channel();
    let (completion_sender, response) = sync_channel(1);
    commands
        .send(WorkerCommand::ReplaceImageImportDevices {
            generation: 11,
            devices: Vec::new(),
            completion_sender,
        })
        .unwrap();
    assert_eq!(
        response.recv_timeout(SETTLE).unwrap().unwrap_err().kind(),
        std::io::ErrorKind::Other
    );
    drop(commands);
    worker.join().unwrap();
}

struct MissingWorkerDevice;

impl crate::RenderDeviceDiscoveryBackend for MissingWorkerDevice {
    type Device = std::fs::File;

    fn open_render_device(&self) -> std::io::Result<Self::Device> {
        panic!("an attached fake worker must not open a device");
    }
}

fn missing_device_exporter()
-> crate::NativeGbmRenderedScanoutBufferDiscoveryExporter<MissingWorkerDevice> {
    let registry = std::sync::Arc::new(super::WorkerRegistry::default());
    let identity = super::DeviceIdentity {
        device: 1,
        inode: 902,
        special_device: 226 << 8,
    };
    let control = std::sync::Arc::new(super::WorkerControl::default());
    let service_control = std::sync::Arc::clone(&control);
    let (commands, receive) = sync_channel(32);
    let thread = registry
        .reserve(identity)
        .unwrap()
        .start(|| {
            std::thread::Builder::new().spawn(move || {
                run_worker::<std::fs::File>(
                    Err(std::io::Error::other("no render device in this test")),
                    Vec::new(),
                    receive,
                    service_control,
                );
            })
        })
        .unwrap();
    let core = std::sync::Arc::new(super::NativeGbmRendererWorkerCore {
        command_sender: commands,
        _thread: std::sync::Mutex::new(thread),
        control,
        inventory_replacement: std::sync::Mutex::new(None),
        release_enqueue_failures: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    });
    let mut exporter =
        crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingWorkerDevice);
    assert!(!exporter.renderer_image_owner_initialized());
    exporter.attach_shared_worker(&core);
    assert!(exporter.renderer_image_owner_initialized());
    exporter
}

#[test]
fn explicit_exporter_shutdown_joins_the_actual_no_device_worker() {
    let exporter = missing_device_exporter();
    assert!(exporter.poll_worker_shutdown().is_err());
    exporter.request_worker_shutdown();
    let deadline = Instant::now() + SETTLE;
    while !exporter.poll_worker_shutdown().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(exporter.poll_worker_shutdown().unwrap());
}

#[test]
fn native_gbm_renderer_worker_defers_then_fails_closed_without_blocking_owner() {
    let mut exporter = missing_device_exporter();
    let target = LiveGbmEglFrameTargetRecord::new(Size {
        width: 16,
        height: 16,
    });
    exporter.set_pending_cpu_frame(sophia_renderer_live::LiveCpuComposedFrame {
        size: target.size,
        stride: 16 * 4,
        format: sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        bytes: vec![0; 16 * 16 * 4].into(),
    });

    let first = exporter.export_rendered_scanout_buffer(target);
    assert_eq!(
        first.status,
        super::LiveRendererScanoutBufferExportStatus::Pending
    );
    assert!(exporter.pending_frame());

    let deadline = std::time::Instant::now() + SETTLE;
    let completed = std::iter::repeat_with(|| ()).find_map(|()| {
        std::thread::sleep(std::time::Duration::from_millis(1));
        let export = exporter.export_rendered_scanout_buffer(target);
        if export.status != super::LiveRendererScanoutBufferExportStatus::Pending
            || std::time::Instant::now() >= deadline
        {
            Some(export)
        } else {
            None
        }
    });
    let completed = completed.expect("unavailable worker should complete without owner blocking");

    assert_eq!(
        completed.status,
        super::LiveRendererScanoutBufferExportStatus::Degraded
    );
    assert!(!exporter.pending_frame());
    let metrics = exporter
        .worker_metrics()
        .expect("worker exporter should expose bounded metrics");
    assert_eq!(metrics.requests, 1);
    assert_eq!(metrics.failures, 1);
    assert_eq!(metrics.hard_stalls, 0);
}

#[test]
fn renderer_image_handoff_settles_an_unsubmitted_worker_frame() {
    let mut exporter = missing_device_exporter();
    let target = LiveGbmEglFrameTargetRecord::new(Size {
        width: 16,
        height: 16,
    });
    exporter.set_pending_cpu_frame(sophia_renderer_live::LiveCpuComposedFrame {
        size: target.size,
        stride: 16 * 4,
        format: sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        bytes: vec![0; 16 * 16 * 4].into(),
    });

    let submitted = exporter.export_rendered_scanout_buffer(target);
    assert_eq!(
        submitted.status,
        super::LiveRendererScanoutBufferExportStatus::Pending
    );
    let error = exporter
        .export_promoted_renderer_image(sophia_renderer_live::LiveRendererImageId::from_raw(1))
        .expect_err("the missing render device must remain a hard failure");

    assert_eq!(
        error,
        sophia_renderer_live::LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable
    );
    assert_ne!(
        error,
        sophia_renderer_live::LiveRendererScanoutBufferExportDetail::WorkerPending
    );
    let metrics = exporter
        .worker_metrics()
        .expect("worker exporter should expose bounded metrics");
    assert_eq!(metrics.requests, 1);
    assert_eq!(metrics.failures, 1);
}
