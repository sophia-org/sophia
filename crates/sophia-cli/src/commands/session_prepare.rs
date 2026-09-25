//! Installed launch argument preparation. This command never starts a session.
use std::{collections::BTreeMap, io::Write, path::Path};

mod environment;
mod standalone;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn try_run(args: &[String]) -> Result<bool> {
    if args.first().map(String::as_str) != Some("session")
        || !matches!(
            args.get(1).map(String::as_str),
            Some("prepare-arguments" | "prepare-environment")
        )
    {
        return Ok(false);
    }
    let (options, extra) = parse(&args[2..])?;
    if args[1] == "prepare-environment" {
        return environment::run(&options, extra).map(|()| true);
    }
    let arguments = prepare(&options, extra)?;
    // A NUL-delimited vector avoids shell evaluation and preserves spaces,
    // newlines and metacharacters. Publish nothing until preparation succeeds.
    let mut output = std::io::stdout().lock();
    output.write_all(b"sophia_session_arguments schema=1 status=prepared\0")?;
    for argument in arguments {
        output.write_all(argument.as_bytes())?;
        output.write_all(b"\0")?;
    }
    Ok(true)
}

fn parse(args: &[String]) -> Result<(BTreeMap<String, String>, &[String])> {
    let mut options = BTreeMap::new();
    let mut extra = &args[args.len()..];
    for (index, argument) in args.iter().enumerate() {
        if argument == "--" {
            extra = &args[index + 1..];
            break;
        }
        let (key, value) = argument
            .strip_prefix("--")
            .and_then(|s| s.split_once('='))
            .ok_or("preparation requires --name=value options, then -- session arguments")?;
        if ![
            "profile",
            "root",
            "state-dir",
            "binary",
            "terminal",
            "terminal-kind",
            "browser",
            "standalone",
            "wm",
            "firefox-profile",
            "tty",
            "firefox-probe",
        ]
        .contains(&key)
            || options.insert(key.to_owned(), value.to_owned()).is_some()
        {
            return Err(format!("unknown or duplicate preparation option: {key}").into());
        }
    }
    Ok((options, extra))
}

fn required<'a>(options: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str> {
    options
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("missing --{key}").into())
}

fn env(name: &str, default: &str) -> Result<String> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        Ok(_) | Err(std::env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(error) => Err(format!("{name}: {error}").into()),
    }
}

fn enabled(name: &str) -> Result<bool> {
    Ok(env(name, "0")? == "1")
}

fn append(args: &mut Vec<String>, values: &[&str]) {
    args.extend(values.iter().map(|value| (*value).to_owned()));
}

fn application_default(args: &mut Vec<String>, role: &str, executable: &str) {
    if !executable.is_empty() {
        args.push(format!("--session-app-default={role}={executable}"));
        args.push(format!("--session-action-default={role}={role}"));
    }
}

fn existing_absolute(value: &str, label: &str) -> Result<()> {
    if !Path::new(value).is_absolute() || !Path::new(value).is_file() {
        return Err(format!("{label} must be an absolute existing file").into());
    }
    Ok(())
}

fn prepare(options: &BTreeMap<String, String>, extra: &[String]) -> Result<Vec<String>> {
    let profile = required(options, "profile")?;
    if !["hagia", "native", "kitty", "standalone"].contains(&profile) {
        return Err("unknown launch profile".into());
    }
    let root = required(options, "root")?;
    let startup = env("SOPHIA_SESSION_STARTUP", "terminal")?;
    if !["terminal", "none"].contains(&startup.as_str()) || startup == "none" && profile != "hagia"
    {
        return Err("terminal-free startup requires Hagia; expected terminal or none".into());
    }
    let truecolor = env("SOPHIA_TRUECOLOR_PROOF", "false")?;
    if !["true", "false"].contains(&truecolor.as_str()) || truecolor == "true" && profile != "hagia"
    {
        return Err("TrueColor proof requires a boolean and the Hagia profile".into());
    }
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
    let defaults = profile == "hagia" && !firefox && truecolor != "true";
    let mut args = vec![
        "session".into(),
        "run".into(),
        "--session-mode=normal".into(),
        format!("--display={}", env("SOPHIA_LIVE_SESSION_DISPLAY", ":77")?),
        "--native-scanout".into(),
    ];
    let devices = env("SOPHIA_OPERATOR_INPUT_DEVICES", "")?;
    args.push(if devices.is_empty() {
        format!(
            "--input-seat={}",
            env("SOPHIA_OPERATOR_INPUT_SEAT", "seat0")?
        )
    } else {
        format!("--input-devices={devices}")
    });
    let config = env("SOPHIA_CORE_CONFIG", "")?;
    if profile != "standalone" && !config.is_empty() {
        existing_absolute(&config, "SOPHIA_CORE_CONFIG")?;
        args.push(format!("--config={config}"));
    }
    if startup != "none" && !defaults {
        args.push("--startup-ready-timeout-ms=8000".into());
    }
    if profile == "standalone" {
        standalone::arguments(&mut args, options)?;
    } else {
        let terminal = required(options, "terminal")?;
        let kind = required(options, "terminal-kind")?;
        if defaults {
            application_default(&mut args, "terminal", terminal);
            if startup != "none" && !terminal.is_empty() {
                args.push("--session-start-default=terminal".into());
            }
        } else {
            terminal_arguments(&mut args, kind, terminal)?;
            if startup != "none" {
                args.push("--session-start=terminal".into());
            }
        }
        let fixture = if truecolor == "true" {
            Some("truecolor_kitty_probe.sh")
        } else if proof("--firefox-m10-primary-proof") {
            Some("firefox_m10_primary_kitty_probe.sh")
        } else if proof("--firefox-m10-selection-proof") {
            Some("firefox_m10_selection_kitty_probe.sh")
        } else if proof("--firefox-m10-proof") || proof("--firefox-m10-lifecycle-proof") {
            Some("firefox_m10_kitty_probe.sh")
        } else {
            None
        };
        if let Some(fixture) = fixture {
            args.push(format!(
                "--session-app-arg=terminal={root}/tools/fixtures/{fixture}"
            ));
        } else if !defaults {
            args.push(format!(
                "--session-app-arg=terminal={}",
                if kind == "kitty" { "--title" } else { "-title" }
            ));
            let title = format!("{}{}", profile[..1].to_uppercase(), &profile[1..]);
            args.push(format!("--session-app-arg=terminal=Sophia {title} TTY3"));
        }
    }
    if profile == "hagia" {
        let desktop = env("SOPHIA_DESKTOP_PROFILE", "")?;
        existing_absolute(&desktop, "SOPHIA_DESKTOP_PROFILE")?;
        args.push(format!("--desktop-profile={desktop}"));
        args.push("--wm-interface=sophia_wm_v1".into());
        let wm = required(options, "wm")?;
        if !wm.is_empty() {
            args.push(format!("--wm-process-default={wm}"));
        }
        let shell = env("SOPHIA_HAGIA_SHELL_BIN", "")?;
        if !shell.is_empty() {
            args.push(format!("--shell-process-default={shell}"));
        }
        if truecolor == "true" {
            args.push(format!(
                "--session-app=palette={}",
                required(options, "binary")?
            ));
            append(
                &mut args,
                &[
                    "--session-app-arg=palette=x-authority-truecolor-palette-client",
                    "--session-start=palette",
                ],
            );
        }
        let browser = required(options, "browser")?;
        if firefox {
            let mut page = format!("file://{root}/tools/fixtures/firefox_m8_local_page.html");
            for (flag, query) in [
                ("dialog", "dialog_only=1"),
                ("primary", "primary_only=1"),
                ("rendering", "rendering_only=1"),
                ("", "promotion_only=1"),
                ("selection", "selection_peer=kitty"),
                ("lifecycle", "lifecycle_only=1"),
            ] {
                let flag = if flag.is_empty() {
                    "--firefox-m10-proof".to_owned()
                } else {
                    format!("--firefox-m10-{flag}-proof")
                };
                if proof(&flag) {
                    page.push('?');
                    page.push_str(query);
                    break;
                }
            }
            args.push(format!("--session-app=browser={browser}"));
            append(
                &mut args,
                &[
                    "--session-app-arg=browser=--no-remote",
                    "--session-app-arg=browser=--new-instance",
                    "--session-app-arg=browser=--profile",
                ],
            );
            args.push(format!(
                "--session-app-arg=browser={}",
                required(options, "firefox-profile")?
            ));
            args.push(format!("--session-app-arg=browser={page}"));
        } else if defaults {
            application_default(&mut args, "browser", browser);
        } else {
            args.push(format!("--session-app=browser={browser}"));
            append(
                &mut args,
                &[
                    "--session-app-arg=browser=--no-remote",
                    "--session-app-arg=browser=--new-instance",
                ],
            );
        }
    } else if profile == "native" {
        args.push("--session-action-app=terminal=terminal".into());
    } else {
        args.push("--exit-when-startup-exits".into());
    }
    if enabled("SOPHIA_ADMIT_XTEST")? {
        args.push("--admit-xtest".into());
    }
    args.extend_from_slice(extra);
    for (variable, flag) in [
        ("SOPHIA_ATOMIC_CURSOR", "--atomic-cursor"),
        ("SOPHIA_ADMIT_XTEST", "--admit-xtest"),
        ("SOPHIA_LEGACY_CURSOR", "--legacy-cursor"),
        ("SOPHIA_DIRECT_CURSOR_PROOF", "--direct-cursor-proof"),
        ("SOPHIA_DIRECT_OVERLAY_PROOF", "--direct-overlay-proof"),
    ] {
        if enabled(variable)? && !args.iter().any(|arg| arg == flag) {
            return Err(format!("session was asked for {flag} and did not receive it").into());
        }
    }
    Ok(args)
}

fn terminal_arguments(args: &mut Vec<String>, kind: &str, terminal: &str) -> Result<()> {
    args.push(format!("--session-app=terminal={terminal}"));
    let values: &[&str] = match kind {
        "kitty" => &[
            "--config",
            "NONE",
            "--override",
            "linux_display_server=x11",
            "--override",
            "background_opacity=1",
            "--override",
            "remember_window_size=no",
        ],
        "xterm" => &["-cm", "-dc"],
        _ => return Err(format!("unsupported terminal kind: {kind}").into()),
    };
    args.extend(
        values
            .iter()
            .map(|value| format!("--session-app-arg=terminal={value}")),
    );
    Ok(())
}
