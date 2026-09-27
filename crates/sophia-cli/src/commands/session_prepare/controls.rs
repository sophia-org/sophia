use super::{BTreeMap, Result, Write, env, validation};

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

pub(super) fn run(options: &BTreeMap<String, String>, extra: &[String]) -> Result<()> {
    if !extra.is_empty() || options.keys().any(|key| key != "profile") {
        return Err("prepare-controls accepts only an optional --profile label".into());
    }
    let profile = if let Some(label) = options.get("profile") {
        // An external recipe supplies a label for state/log paths, not a
        // selector for applications, startup policy or proof behavior.
        if label.is_empty()
            || label.len() > 64
            || !label.as_bytes()[0].is_ascii_alphanumeric()
            || !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err("profile label requires 1-64 ASCII letters, digits, dot, dash or underscore, starting with a letter or digit".into());
        }
        label.clone()
    } else {
        legacy_profile()?
    };
    let watchdog = env("SOPHIA_SESSION_WATCHDOG_SECONDS", "")?;
    if !watchdog.is_empty() {
        validation::positive("SOPHIA_SESSION_WATCHDOG_SECONDS", "")?;
    }
    let timeout = validation::positive("SOPHIA_INPUT_GUARD_ARM_TIMEOUT_SECONDS", "30")?;
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

// Kept until the external recipe gate covers the old invocation without a
// profile argument. The explicit-label path has no recipe dependencies.
fn legacy_profile() -> Result<String> {
    let profile = choice(
        "SOPHIA_TTY_PROFILE",
        "",
        &["hagia", "native", "kitty", "standalone"],
    )?;
    let startup = choice("SOPHIA_SESSION_STARTUP", "terminal", &["terminal", "none"])?;
    if startup == "none" && profile != "hagia" {
        return Err("terminal-free startup requires Hagia".into());
    }
    let truecolor = choice("SOPHIA_TRUECOLOR_PROOF", "false", &["true", "false"])?;
    if truecolor == "true" && profile != "hagia" {
        return Err("TrueColor proof requires Hagia".into());
    }
    Ok(profile)
}
