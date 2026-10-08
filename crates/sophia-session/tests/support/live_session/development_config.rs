use super::super::*;
use super::isolated_session_config;

#[test]
fn development_arguments_keep_native_input_empty_and_daily_native_guarded() {
    if std::env::var_os("SOPHIA_DEVELOPMENT_ARGV_CHILD").is_none() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "live_session::tests::session_config_tests::development_config::development_arguments_keep_native_input_empty_and_daily_native_guarded", "--nocapture"])
            .env("SOPHIA_DEVELOPMENT_ARGV_CHILD", "1")
            .env("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE", "1")
            .stdin(std::process::Stdio::null())
            .output().unwrap();
        assert!(child.status.success(), "{child:?}");
        return;
    }
    let args: Vec<String> = [
        "--native-scanout",
        "--no-input",
        "--development-seat=seat-sophia-dev",
        "--max-runtime-ms=60000",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let config = isolated_session_config(&args).unwrap();
    assert!(config.native_scanout);
    assert!(config.development_seat.is_some());
    assert!(config.input_devices.is_empty());
    assert!(config.input_seat.is_none());
    // Exercise the real input-opening path with no seat opener. Any fallback
    // to a profile/seat must fail or return Some here, even without hardware.
    let map =
        sophia_backend_live::NativeLibinputDeviceMap::new(sophia_protocol::SeatId::from_raw(1));
    assert!(
        open_unattached_session_physical_input(&config, map, None)
            .unwrap()
            .is_none()
    );
    for remove in [
        "--native-scanout",
        "--no-input",
        "--development-seat=seat-sophia-dev",
        "--max-runtime-ms=60000",
    ] {
        let changed: Vec<_> = args
            .iter()
            .filter(|arg| arg.as_str() != remove)
            .cloned()
            .collect();
        assert!(
            isolated_session_config(&changed).is_err(),
            "removing {remove} must refuse"
        );
    }
    for extra in [
        "--input-seat=seat0",
        "--input-devices=/dev/input/event0",
        "--expect-physical-pointer",
        "--expect-physical-text=abc",
    ] {
        let mut changed = args.clone();
        changed.push(extra.into());
        assert!(
            isolated_session_config(&changed).is_err(),
            "{extra} must refuse"
        );
    }
}

#[test]
fn development_refusal_precedes_endpoints_and_the_seat_broker() {
    if std::env::var_os("SOPHIA_DEVELOPMENT_REFUSAL_CHILD").is_none() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "live_session::tests::session_config_tests::development_config::development_refusal_precedes_endpoints_and_the_seat_broker", "--nocapture"])
            .env("SOPHIA_DEVELOPMENT_REFUSAL_CHILD", "1")
            .env("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE", "1")
            .env("LIBSEAT_BACKEND", "noop")
            .stdin(std::process::Stdio::null()).output().unwrap();
        assert!(child.status.success(), "{child:?}");
        return;
    }
    let display = 42000 + std::process::id() % 10000;
    let core = super::isolated_core_config_argument();
    let desktop = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/fixtures/mixed_output_probe.kdl");
    let args = vec![
        core,
        format!("--desktop-profile={}", desktop.display()),
        format!("--display=:{display}"),
        "--native-scanout".into(),
        "--no-input".into(),
        "--development-seat=seat-sophia-dev".into(),
        "--max-runtime-ms=60000".into(),
    ];
    let socket = std::path::PathBuf::from(format!("/tmp/.X11-unix/X{display}"));
    assert!(!socket.exists());
    let mut stage = crate::diagnostics::SessionRunStage::Startup;
    let error = run_persistent_xterm_session(&args, &mut stage).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires LIBSEAT_BACKEND=logind"),
        "{error}"
    );
    assert!(
        !socket.exists(),
        "refused development startup must not bind the display"
    );
}
