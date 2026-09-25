use super::{BTreeMap, Result, Write, discovery, env};

fn choice(name: &str, default: &str, choices: &[&str]) -> Result<String> {
    let value = env(name, default)?;
    if !choices.contains(&value.as_str()) {
        return Err(format!("{name} must be {}", choices.join(" or ")).into());
    }
    Ok(value)
}

fn identity(name: &str) -> Result<String> {
    let value = env(name, "unknown")?;
    Ok(
        if value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            value
        } else {
            "unknown".into()
        },
    )
}

pub(super) fn run(_options: &BTreeMap<String, String>, _extra: &[String]) -> Result<()> {
    let profile = choice(
        "SOPHIA_TTY_PROFILE",
        "",
        &["hagia", "native", "kitty", "standalone"],
    )?;
    let startup = choice("SOPHIA_SESSION_STARTUP", "terminal", &["terminal", "none"])?;
    if startup == "none" && profile != "hagia" {
        return Err("terminal-free startup requires Hagia".into());
    }
    let watchdog = env("SOPHIA_SESSION_WATCHDOG_SECONDS", "")?;
    if !watchdog.is_empty() {
        discovery::positive("SOPHIA_SESSION_WATCHDOG_SECONDS", "")?;
    }
    let timeout = discovery::positive("SOPHIA_INPUT_GUARD_ARM_TIMEOUT_SECONDS", "30")?;
    let seconds = timeout.parse::<u32>()?;
    if seconds > 300 {
        return Err("SOPHIA_INPUT_GUARD_ARM_TIMEOUT_SECONDS must be in 1..300".into());
    }
    let arming = choice(
        "SOPHIA_INPUT_GUARD_ARMING",
        "manual",
        &["manual", "automatic"],
    )?;
    let handoff = choice(
        "SOPHIA_SESSION_HANDOFF",
        "display_manager",
        &["display_manager", "cycle_runner"],
    )?;
    let truecolor = choice("SOPHIA_TRUECOLOR_PROOF", "false", &["true", "false"])?;
    if truecolor == "true" && profile != "hagia" {
        return Err("TrueColor proof requires Hagia".into());
    }
    let values = [
        profile,
        watchdog,
        timeout,
        (seconds * 20).to_string(),
        arming,
        handoff,
        identity("SOPHIA_INSTALLED_VERSION")?,
        identity("SOPHIA_INSTALLED_COMMIT")?,
    ];
    let mut output = std::io::stdout().lock();
    output.write_all(b"sophia_session_controls schema=1 status=prepared\0")?;
    for value in values {
        output.write_all(value.as_bytes())?;
        output.write_all(b"\0")?;
    }
    Ok(())
}
