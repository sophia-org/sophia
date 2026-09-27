use super::{BTreeMap, Result, Write, enabled, env, required};

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
    if !extra.is_empty() || options.keys().any(|key| key != "tty") {
        return Err("prepare-environment accepts only --tty".into());
    }
    let mut values = vec![
        "SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1".to_owned(),
        format!("SOPHIA_SESSION_TTY={}", required(options, "tty")?),
    ];
    if env("SOPHIA_SESSION_VERBOSE_TRACE", "false")? == "true" {
        for name in [
            "SOPHIA_LIVE_SESSION_DIAGNOSTIC",
            "SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE",
            "SOPHIA_X11_AUTHORITY_TRACE",
        ] {
            values.push(format!("{name}={}", trace(name, "1")?));
        }
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
