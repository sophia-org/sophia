use super::{BTreeMap, Result, Write, append, enabled, env, required};

// Unlike the ordinary fallback reader, trace defaults preserve an explicitly
// empty value: the Bash contract uses ${NAME-default}, not ${NAME:-default}.
fn trace(name: &str, default: &str) -> Result<String> {
    match std::env::var(name) {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(error) => Err(format!("{name}: {error}").into()),
    }
}

pub(super) fn run(options: &BTreeMap<String, String>, extra: &[String]) -> Result<()> {
    let proof = |name: &str| extra.iter().any(|arg| arg == name);
    let firefox = [
        "--firefox-m10-proof",
        "--firefox-m10-rendering-proof",
        "--firefox-m10-dialog-proof",
        "--firefox-m10-primary-proof",
        "--firefox-m10-selection-proof",
        "--firefox-m10-lifecycle-proof",
    ]
    .iter()
    .any(|name| proof(name));
    let mut values = vec![
        "SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1".to_owned(),
        format!("SOPHIA_SESSION_TTY={}", required(options, "tty")?),
    ];
    if firefox {
        let mut slice = "promotion";
        for name in ["selection", "primary", "dialog", "rendering", "lifecycle"] {
            if proof(&format!("--firefox-m10-{name}-proof")) {
                slice = name;
                break;
            }
        }
        values.push(format!(
            "SOPHIA_FIREFOX_M10_KITTY_PROBE_DIR={}",
            required(options, "firefox-probe")?
        ));
        values.push(format!("SOPHIA_FIREFOX_M10_PROOF_SLICE={slice}"));
        append(
            &mut values,
            &[
                "GDK_BACKEND=x11",
                "GTK_USE_PORTAL=0",
                "MOZ_ENABLE_WAYLAND=0",
                "MOZ_FORCE_DISABLE_E10S=1",
                "MOZ_USE_XINPUT2=1",
            ],
        );
    }
    if env("SOPHIA_SESSION_VERBOSE_TRACE", "false")? == "true" {
        for name in [
            "SOPHIA_LIVE_SESSION_DIAGNOSTIC",
            "SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE",
            "SOPHIA_X11_AUTHORITY_TRACE",
        ] {
            values.push(format!("{name}={}", trace(name, "1")?));
        }
    }
    if proof("--firefox-m10-rendering-proof") {
        values.push(format!(
            "SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE={}",
            trace("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE", "final-regions")?
        ));
        values.push(format!(
            "SOPHIA_X11_PIXEL_TRACE={}",
            trace("SOPHIA_X11_PIXEL_TRACE", "1")?
        ));
    }
    let bus = env("DBUS_SESSION_BUS_ADDRESS", "")?;
    let mode = if enabled("SOPHIA_ISOLATE_SESSION_BUS")? {
        values.push("DBUS_SESSION_BUS_ADDRESS=unix:path=/dev/null".into());
        "isolated"
    } else if !bus.is_empty() && bus != "unix:path=/dev/null" {
        "inherited"
    } else if std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).any(|path| {
        let executable = path.join("dbus-run-session");
        executable.is_file() && rustix::fs::access(executable, rustix::fs::Access::EXEC_OK).is_ok()
    }) {
        "session_scoped"
    } else {
        "unavailable"
    };
    let mut output = std::io::stdout().lock();
    output.write_all(b"sophia_session_environment schema=1 status=prepared\0")?;
    output.write_all(mode.as_bytes())?;
    output.write_all(b"\0")?;
    for value in values {
        output.write_all(value.as_bytes())?;
        output.write_all(b"\0")?;
    }
    Ok(())
}
