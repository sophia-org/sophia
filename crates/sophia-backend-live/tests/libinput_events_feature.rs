#![cfg(feature = "libinput-events")]

use std::fs;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use sophia_backend_live::{
    CompositorBackendTickInput, DeviceId, FakeLiveLibinputEventReader, InputEventPacket,
    LibinputDeviceDescriptor, LibinputDeviceKind, LibinputEventIngest, LibinputEventSource,
    LibinputNativeEventAdapterReport, LibinputNativeEventAdapterStatus,
    LibinputNativeEventReadReport, LibinputNativeEventReadStatus, LibinputPhysicalInputAdapter,
    LiveBackendConfig, LiveHardwareValidationGateReport, LiveHardwareValidationGateStatus,
    LiveHardwareValidationSmokeReport, LiveHardwareValidationSmokeStatus,
    LiveHardwareValidationTarget, LiveInputReadinessGateReport, LiveInputReadinessGateStatus,
    LiveInputReadinessGatedPoller, NativeLibinputDeviceMap, NativeLibinputEventPoller,
    NativeLibinputEventReader, NativeLibinputOpenError, NativeLibinputPointerPolicy,
    NativeLibinputPolicyReport, NonBlockingInputPoller, SeatId, discover_live_backend,
    native_libinput_event_adapter_report, open_native_libinput_path_poller,
    real_libinput_events_validation_gate, real_libinput_events_validation_smoke_report,
    resolve_native_libinput_device_path,
};
use sophia_protocol::{InputEventKind, Point};

#[test]
fn native_pointer_policy_validation_is_bounded() {
    assert!(NativeLibinputPointerPolicy::default().validate().is_some());
    assert!(
        NativeLibinputPointerPolicy {
            scroll_factor: 10.1,
            ..NativeLibinputPointerPolicy::default()
        }
        .validate()
        .is_none()
    );
    assert!(
        NativeLibinputPointerPolicy {
            accel_speed: Some(f64::NAN),
            ..NativeLibinputPointerPolicy::default()
        }
        .validate()
        .is_none()
    );
}

#[test]
fn native_pointer_scroll_scaling_rounds_and_saturates() {
    let half = NativeLibinputPointerPolicy {
        scroll_factor: 0.5,
        ..NativeLibinputPointerPolicy::default()
    };
    assert_eq!(half.scale_scroll_v120(120.0), 60);
    let maximum = NativeLibinputPointerPolicy {
        scroll_factor: 10.0,
        ..NativeLibinputPointerPolicy::default()
    };
    assert_eq!(maximum.scale_scroll_v120(f64::MAX), i32::MAX);
    assert_eq!(maximum.scale_scroll_v120(f64::MIN), i32::MIN);
}

#[test]
fn native_libinput_event_adapter_skeleton_reports_ready_without_opening_devices() {
    assert_eq!(
        native_libinput_event_adapter_report(),
        LibinputNativeEventAdapterReport {
            status: LibinputNativeEventAdapterStatus::SkeletonReady,
        }
    );
}

#[test]
fn native_libinput_path_poller_fails_closed_without_exposing_paths() {
    let devices = NativeLibinputDeviceMap::new(SeatId::from_raw(1));
    assert_eq!(
        open_native_libinput_path_poller(&[], devices, 64).unwrap_err(),
        NativeLibinputOpenError::NoDevices
    );
    assert_eq!(
        open_native_libinput_path_poller(&[PathBuf::from("relative-event")], devices, 64)
            .unwrap_err(),
        NativeLibinputOpenError::InvalidDevicePath
    );
    assert_eq!(
        open_native_libinput_path_poller(
            &[PathBuf::from("/definitely/missing/sophia-input-event")],
            devices,
            64,
        )
        .unwrap_err(),
        NativeLibinputOpenError::DeviceUnavailable
    );
}

#[test]
fn native_udev_poller_rejects_invalid_seat_names_before_discovery() {
    let devices = NativeLibinputDeviceMap::new(SeatId::from_raw(1))
        .with_keyboard_device(DeviceId::from_raw(2))
        .with_pointer_device(DeviceId::from_raw(3));
    assert_eq!(
        sophia_backend_live::open_native_libinput_udev_poller("", devices, 64).unwrap_err(),
        NativeLibinputOpenError::SeatAssignmentFailed
    );
    assert_eq!(
        sophia_backend_live::open_native_libinput_udev_poller(
            "seat-name-that-is-deliberately-longer-than-sixty-four-ascii-characters",
            devices,
            64,
        )
        .unwrap_err(),
        NativeLibinputOpenError::SeatAssignmentFailed
    );
}

#[test]
fn native_libinput_device_paths_resolve_stable_symlinks_before_libinput_admission() {
    let root = std::env::temp_dir().join(format!(
        "sophia-backend-live-libinput-symlink-{}",
        std::process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(&root).unwrap();
    let event = root.join("event0");
    fs::write(&event, []).unwrap();
    let by_path = root.join("platform-test-event-kbd");
    std::os::unix::fs::symlink(&event, &by_path).unwrap();

    assert_eq!(
        resolve_native_libinput_device_path(&by_path).unwrap(),
        event
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn real_libinput_event_validation_gate_is_explicit_and_reduced() {
    let skipped = LiveHardwareValidationGateReport::from_env_presence(
        LiveHardwareValidationTarget::LibinputEvents,
        false,
    );
    assert_eq!(
        skipped,
        LiveHardwareValidationGateReport {
            target: LiveHardwareValidationTarget::LibinputEvents,
            status: LiveHardwareValidationGateStatus::SkippedOptInRequired,
        }
    );
    assert!(!skipped.is_requested());
    assert_eq!(
        skipped.target.env_var(),
        "SOPHIA_RUN_REAL_LIBINPUT_EVENTS_SMOKE"
    );

    let requested = LiveHardwareValidationGateReport::from_env_presence(
        LiveHardwareValidationTarget::LibinputEvents,
        true,
    );
    assert_eq!(
        requested.status,
        LiveHardwareValidationGateStatus::Requested
    );
    assert!(requested.is_requested());

    assert_eq!(
        real_libinput_events_validation_gate().target,
        LiveHardwareValidationTarget::LibinputEvents
    );
}

#[test]
fn real_libinput_event_validation_smoke_fails_closed_without_native_reader() {
    let skipped = LiveHardwareValidationSmokeReport::fail_closed_from_gate(
        LiveHardwareValidationGateReport::from_env_presence(
            LiveHardwareValidationTarget::LibinputEvents,
            false,
        ),
    );
    assert_eq!(
        skipped,
        LiveHardwareValidationSmokeReport {
            target: LiveHardwareValidationTarget::LibinputEvents,
            status: LiveHardwareValidationSmokeStatus::SkippedOptInRequired,
        }
    );

    let requested = LiveHardwareValidationSmokeReport::fail_closed_from_gate(
        LiveHardwareValidationGateReport::from_env_presence(
            LiveHardwareValidationTarget::LibinputEvents,
            true,
        ),
    );
    assert_eq!(
        requested,
        LiveHardwareValidationSmokeReport {
            target: LiveHardwareValidationTarget::LibinputEvents,
            status: LiveHardwareValidationSmokeStatus::BackendUnavailable,
        }
    );

    assert_eq!(
        real_libinput_events_validation_smoke_report().target,
        LiveHardwareValidationTarget::LibinputEvents
    );
}

#[test]
fn native_libinput_event_poller_reads_bounded_events() {
    let mut poller = NativeLibinputEventPoller::new(
        FakeLiveLibinputEventReader::new([
            motion_event(1, 10.0, 20.0),
            motion_event(2, 11.0, 21.0),
        ]),
        1,
    );

    let first = poller.poll_ready().expect("fake input read should succeed");
    assert_eq!(first, vec![motion_event(1, 10.0, 20.0)]);
    assert_eq!(
        poller.last_read_report(),
        LibinputNativeEventReadReport {
            status: LibinputNativeEventReadStatus::EventsRead,
            events_read: 1,
            queued_remaining: 1,
        }
    );
    assert_eq!(poller.reader().queued_len(), 1);

    let second = poller.poll_ready().expect("fake input read should succeed");
    assert_eq!(second, vec![motion_event(2, 11.0, 21.0)]);
    assert_eq!(poller.reader().queued_len(), 0);

    let empty = poller.poll_ready().expect("empty fake read should succeed");
    assert!(empty.is_empty());
    assert_eq!(
        poller.last_read_report(),
        LibinputNativeEventReadReport::idle()
    );
}

#[test]
fn native_libinput_event_poller_reports_reduced_read_failure() {
    let mut poller = NativeLibinputEventPoller::new(
        FakeLiveLibinputEventReader::new([motion_event(1, 10.0, 20.0)]),
        4,
    );
    poller.reader_mut().fail_next_read();

    assert!(poller.poll_ready().is_err());
    assert_eq!(
        poller.last_read_report(),
        LibinputNativeEventReadReport::read_failed()
    );
    assert_eq!(poller.reader().queued_len(), 1);
}

#[test]
fn input_readiness_gate_skips_polling_until_ready_token_is_observed() {
    let poller = NativeLibinputEventPoller::new(
        FakeLiveLibinputEventReader::new([
            motion_event(1, 10.0, 20.0),
            motion_event(2, 11.0, 21.0),
        ]),
        1,
    );
    let mut gated = LiveInputReadinessGatedPoller::new(poller);

    let idle = gated
        .poll_ready()
        .expect("idle gate should not call inner poller");

    assert!(idle.is_empty());
    assert_eq!(
        gated.last_gate_report(),
        LiveInputReadinessGateReport {
            status: LiveInputReadinessGateStatus::Idle,
        }
    );
    assert_eq!(gated.inner().reader().queued_len(), 2);

    gated.observe_ready();
    let ready = gated
        .poll_ready()
        .expect("ready gate should call inner poller once");

    assert_eq!(ready, vec![motion_event(1, 10.0, 20.0)]);
    assert_eq!(
        gated.last_gate_report(),
        LiveInputReadinessGateReport::polled()
    );
    assert_eq!(gated.inner().reader().queued_len(), 1);
    assert!(!gated.ready());

    let second_idle = gated
        .poll_ready()
        .expect("consumed readiness should not poll twice");

    assert!(second_idle.is_empty());
    assert_eq!(
        gated.last_gate_report(),
        LiveInputReadinessGateReport::idle()
    );
    assert_eq!(gated.inner().reader().queued_len(), 1);
}

#[test]
fn input_readiness_gate_reports_reduced_read_failure() {
    let poller = NativeLibinputEventPoller::new(
        FakeLiveLibinputEventReader::new([motion_event(1, 10.0, 20.0)]),
        4,
    );
    let mut gated = LiveInputReadinessGatedPoller::new(poller);
    gated.inner_mut().reader_mut().fail_next_read();
    gated.observe_ready();

    assert!(gated.poll_ready().is_err());
    assert_eq!(
        gated.last_gate_report(),
        LiveInputReadinessGateReport::read_failed()
    );
    assert!(!gated.ready());
    assert_eq!(gated.inner().reader().queued_len(), 1);
}

#[test]
fn native_libinput_event_reader_idles_without_exposing_native_identity() {
    let reader = NativeLibinputEventReader::new_with_policy(
        input::Libinput::new_from_path(RejectingLibinputInterface),
        NativeLibinputDeviceMap::new(SeatId::from_raw(1))
            .with_pointer_device(DeviceId::from_raw(2))
            .with_keyboard_device(DeviceId::from_raw(3)),
        NativeLibinputPolicyReport {
            devices_added: 3,
            tap_capable: 1,
            tap_enabled: 1,
            ..NativeLibinputPolicyReport::default()
        },
    );
    let mut poller = NativeLibinputEventPoller::new(reader, 4);

    let events = poller
        .poll_ready()
        .expect("empty path libinput context should reduce to idle");

    assert!(events.is_empty());
    assert_eq!(
        poller.last_read_report(),
        LibinputNativeEventReadReport::idle()
    );
    assert_eq!(
        poller.reader().devices(),
        NativeLibinputDeviceMap::new(SeatId::from_raw(1))
            .with_pointer_device(DeviceId::from_raw(2))
            .with_keyboard_device(DeviceId::from_raw(3))
    );
    assert!(poller.reader().device_inventory().is_empty());
    assert_eq!(poller.reader().retained_len(), 0);
    assert_eq!(
        poller.reader().policy_report(),
        NativeLibinputPolicyReport {
            devices_added: 3,
            tap_capable: 1,
            tap_enabled: 1,
            ..NativeLibinputPolicyReport::default()
        }
    );
}

#[test]
fn native_libinput_event_poller_feeds_engine_input_adapter_contract() {
    let mut source = LibinputEventSource::new();
    source.register_device(LibinputDeviceDescriptor {
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Pointer,
    });
    let poller = NativeLibinputEventPoller::new(
        FakeLiveLibinputEventReader::new([
            motion_event(1, 10.0, 20.0),
            unknown_device_motion_event(2, 11.0, 21.0),
        ]),
        4,
    );
    let mut adapter = LibinputPhysicalInputAdapter::new(poller, source);

    let report = adapter
        .poll_once()
        .expect("fake native poller should feed adapter");

    assert_eq!(report.polled, 2);
    assert_eq!(report.accepted, 1);
    assert_eq!(report.rejected, vec![LibinputEventIngest::UnknownDevice]);
    assert_eq!(adapter.source().pending_len(), 1);
}

#[test]
fn live_runtime_assembly_runs_tick_with_native_shaped_input_poller() {
    let root = ready_drm_sysfs_fixture("native-input-runtime");
    let config = LiveBackendConfig::new(&root).with_input_device(LibinputDeviceDescriptor {
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Pointer,
    });
    let poller = NativeLibinputEventPoller::new(
        FakeLiveLibinputEventReader::new([motion_event(1, 10.0, 20.0)]),
        4,
    );
    let mut assembly = discover_live_backend(&config)
        .into_live_runtime_assembly(poller)
        .expect("ready startup should accept native-shaped input poller");

    let report = assembly
        .run_tick(CompositorBackendTickInput::default())
        .expect("native-shaped input poller should drive runtime tick");

    assert_eq!(report.engine.input_poll.polled, 1);
    assert_eq!(report.engine.input_poll.accepted, 1);
    assert!(report.engine.input_poll.rejected.is_empty());
    assert_eq!(assembly.assembly().input().source().pending_len(), 1);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn live_runtime_assembly_uses_input_readiness_gate_without_blocking_ticks() {
    let root = ready_drm_sysfs_fixture("native-input-readiness-gate-runtime");
    let config = LiveBackendConfig::new(&root).with_input_device(LibinputDeviceDescriptor {
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Pointer,
    });
    let poller = LiveInputReadinessGatedPoller::new(NativeLibinputEventPoller::new(
        FakeLiveLibinputEventReader::new([motion_event(1, 10.0, 20.0)]),
        4,
    ));
    let mut assembly = discover_live_backend(&config)
        .into_live_runtime_assembly(poller)
        .expect("ready startup should accept gated native-shaped input poller");

    let idle_tick = assembly
        .run_tick(CompositorBackendTickInput::default())
        .expect("runtime tick should continue when input is not ready");

    assert_eq!(idle_tick.engine.input_poll.polled, 0);
    assert_eq!(idle_tick.engine.input_poll.accepted, 0);
    assert_eq!(assembly.assembly().input().source().pending_len(), 0);
    assert_eq!(
        assembly.assembly().input().poller().last_gate_report(),
        LiveInputReadinessGateReport::idle()
    );
    assert_eq!(
        assembly
            .assembly()
            .input()
            .poller()
            .inner()
            .reader()
            .queued_len(),
        1
    );

    assembly
        .assembly_mut()
        .input_mut()
        .poller_mut()
        .observe_ready();
    let ready_tick = assembly
        .run_tick(CompositorBackendTickInput::default())
        .expect("runtime tick should poll input after readiness is observed");

    assert_eq!(ready_tick.engine.input_poll.polled, 1);
    assert_eq!(ready_tick.engine.input_poll.accepted, 1);
    assert!(ready_tick.engine.input_poll.rejected.is_empty());
    assert_eq!(assembly.assembly().input().source().pending_len(), 1);
    assert_eq!(
        assembly.assembly().input().poller().last_gate_report(),
        LiveInputReadinessGateReport::polled()
    );

    fs::remove_dir_all(root).unwrap();
}

fn motion_event(serial: u64, x: f64, y: f64) -> InputEventPacket {
    InputEventPacket {
        serial,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        time_msec: serial * 10,
        kind: InputEventKind::PointerMotion,
        global_position: Some(Point { x, y }),
        target_surface: None,
        local_position: None,
    }
}

fn unknown_device_motion_event(serial: u64, x: f64, y: f64) -> InputEventPacket {
    InputEventPacket {
        device: DeviceId::from_raw(99),
        ..motion_event(serial, x, y)
    }
}

fn ready_drm_sysfs_fixture(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "sophia-backend-live-libinput-events-{name}-{}",
        std::process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    let connector = root.join("card0-HDMI-A-1");
    fs::create_dir_all(&connector).unwrap();
    write_fixture_file(&connector, "status", "connected\n");
    write_fixture_file(&connector, "modes", "1920x1080\n");
    write_fixture_file(&connector, "connector_id", "42\n");
    write_fixture_file(&connector, "crtc_id", "99\n");
    root
}

fn write_fixture_file(root: &Path, name: &str, value: &str) {
    fs::write(root.join(name), value).unwrap();
}

struct RejectingLibinputInterface;

impl input::LibinputInterface for RejectingLibinputInterface {
    fn open_restricted(&mut self, _path: &Path, _flags: i32) -> Result<OwnedFd, i32> {
        Err(1)
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        drop(fd);
    }
}
