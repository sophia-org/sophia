use super::{BTreeMap, Result, append, enabled, env, existing_absolute, required};

fn positive(name: &str, default: &str) -> Result<String> {
    let value = env(name, default)?;
    if !value.starts_with(['1', '2', '3', '4', '5', '6', '7', '8', '9'])
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!("{name} must be a positive integer").into());
    }
    Ok(value)
}

pub(super) fn arguments(args: &mut Vec<String>, options: &BTreeMap<String, String>) -> Result<()> {
    let direct = enabled("SOPHIA_ENABLE_DIRECT_SCANOUT")?;
    if direct {
        let state = required(options, "state-dir")?;
        for (flag, file) in [
            ("config", "standalone-core.kdl"),
            ("desktop-profile", "standalone-desktop.kdl"),
        ] {
            let path = format!("{state}/{file}");
            existing_absolute(&path, "staged standalone configuration")?;
            args.push(format!("--{flag}={path}"));
        }
        if enabled("SOPHIA_DIRECT_CURSOR_PROOF")? {
            args.push("--direct-cursor-proof".into());
        }
        if enabled("SOPHIA_DIRECT_OVERLAY_PROOF")? {
            args.push("--direct-overlay-proof".into());
            let ticks = env("SOPHIA_DIRECT_OVERLAY_HOLD_TICKS", "")?;
            if !ticks.is_empty() {
                args.push(format!("--direct-overlay-hold-ticks={ticks}"));
            }
        }
    } else {
        args.push("--no-config".into());
    }
    if enabled("SOPHIA_ATOMIC_CURSOR")? {
        args.push("--atomic-cursor".into());
    }
    if enabled("SOPHIA_LEGACY_CURSOR")? {
        args.push("--legacy-cursor".into());
    }
    // Bounded probes keep full diagnostic records rather than daily-session
    // reduced evidence; the independent watchdog still owns termination.
    let watchdog = positive("SOPHIA_SESSION_WATCHDOG_SECONDS", "600")?.parse::<u64>()?;
    let maximum = watchdog
        .checked_add(30)
        .and_then(|n| n.checked_mul(1000))
        .ok_or("standalone runtime bound overflow")?;
    args.push(format!(
        "--session-app=standalone={}",
        required(options, "standalone")?
    ));
    append(
        args,
        &["--session-start=standalone", "--exit-when-startup-exits"],
    );
    args.push(format!("--max-runtime-ms={maximum}"));
    let workload = env("SOPHIA_STANDALONE_WORKLOAD", "vkcube")?;
    if !["vkcube", "kitty", "glxgears", "xterm"].contains(&workload.as_str()) {
        return Err("unknown standalone workload".into());
    }
    if workload == "vkcube" {
        append(
            args,
            &[
                "--session-app-arg=standalone=--wsi",
                "--session-app-arg=standalone=xcb",
            ],
        );
    }
    if workload == "kitty" {
        let width = positive("SOPHIA_STANDALONE_WIDTH", "2560")?;
        let height = positive("SOPHIA_STANDALONE_HEIGHT", "1440")?;
        append(
            args,
            &[
                "--session-app-arg=standalone=--config",
                "--session-app-arg=standalone=NONE",
            ],
        );
        for value in [
            "linux_display_server=x11".into(),
            "background_opacity=1".into(),
            "remember_window_size=no".into(),
            format!("initial_window_width={width}"),
            format!("initial_window_height={height}"),
            "confirm_os_window_close=0".into(),
        ] {
            args.push("--session-app-arg=standalone=--override".into());
            args.push(format!("--session-app-arg=standalone={value}"));
        }
        append(
            args,
            &[
                "--session-app-arg=standalone=sh",
                "--session-app-arg=standalone=-c",
            ],
        );
        args.push(format!(
            "--session-app-arg=standalone=sleep {}",
            env("SOPHIA_STANDALONE_HOLD_SECONDS", "20")?
        ));
    }
    let frames = env(
        "SOPHIA_STANDALONE_FRAME_COUNT",
        if direct && workload == "vkcube" {
            "600"
        } else {
            ""
        },
    )?;
    if !frames.is_empty() {
        if workload != "vkcube" {
            return Err("frame count requires vkcube".into());
        }
        let frames = positive("SOPHIA_STANDALONE_FRAME_COUNT", &frames)?;
        let width = positive(
            "SOPHIA_STANDALONE_WIDTH",
            if direct { "2560" } else { "500" },
        )?;
        let height = positive(
            "SOPHIA_STANDALONE_HEIGHT",
            if direct { "1440" } else { "500" },
        )?;
        let mode = env("SOPHIA_STANDALONE_PRESENT_MODE", "2")?;
        if !["0", "1", "2", "3"].contains(&mode.as_str()) {
            return Err("present mode must be 0 through 3".into());
        }
        for (flag, value) in [
            ("--c", frames),
            ("--width", width),
            ("--height", height),
            ("--present_mode", mode),
        ] {
            args.push(format!("--session-app-arg=standalone={flag}"));
            args.push(format!("--session-app-arg=standalone={value}"));
        }
    }
    Ok(())
}
