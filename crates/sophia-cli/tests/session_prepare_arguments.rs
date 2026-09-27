//! Desktop recipes and their frozen oracles live in niltempus. Sophia keeps
//! only the product-neutral environment and option-validation contract.
use std::process::Command;

#[test]
fn preparation_rejects_unknown_and_duplicate_options_without_output() {
    for (verb, options) in [
        ("prepare-controls", vec!["--unknown=true"]),
        ("prepare-controls", vec!["--profile=one", "--profile=two"]),
        ("prepare-controls", vec!["--profile=../escape"]),
        (
            "prepare-environment",
            vec!["--tty=/dev/tty3", "--", "application"],
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sophia"))
            .args(["session", verb])
            .args(options)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn environment_preserves_trace_overrides_and_bus_ownership() {
    for (bus, isolate, expected) in [
        ("unix:path=/private bus/socket", "0", "inherited"),
        ("unix:path=/private bus/socket", "1", "isolated"),
        ("", "0", "unavailable"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sophia"))
            .args(["session", "prepare-environment", "--tty=/dev/tty37"])
            .env_clear()
            .env("PATH", "/nonexistent")
            .env("DBUS_SESSION_BUS_ADDRESS", bus)
            .env("SOPHIA_ISOLATE_SESSION_BUS", isolate)
            .env("SOPHIA_SESSION_VERBOSE_TRACE", "true")
            .env("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE", "")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let fields = std::str::from_utf8(&output.stdout)
            .unwrap()
            .split('\0')
            .collect::<Vec<_>>();
        assert_eq!(
            fields[0],
            "sophia_session_environment schema=1 status=prepared"
        );
        assert_eq!(fields[1], expected);
        assert!(fields.contains(&"SOPHIA_SESSION_TTY=/dev/tty37"));
        assert!(fields.contains(&"SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1"));
        assert!(fields.contains(&"SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE="));
        assert!(fields.contains(&"SOPHIA_X11_AUTHORITY_TRACE=1"));
        assert_eq!(
            fields.contains(&"DBUS_SESSION_BUS_ADDRESS=unix:path=/dev/null"),
            isolate == "1"
        );
    }
}
