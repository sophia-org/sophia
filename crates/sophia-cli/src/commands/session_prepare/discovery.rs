use super::{BTreeMap, Result, Write, env, required};
use std::path::Path;

pub(super) fn executable(path: &Path) -> bool {
    path.is_file() && rustix::fs::access(path, rustix::fs::Access::EXEC_OK).is_ok()
}

fn find(names: &[&str]) -> String {
    for name in names {
        for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
            let path = directory.join(name);
            if executable(&path) {
                return path.to_string_lossy().into_owned();
            }
        }
    }
    String::new()
}

fn selected(variable: &str, defaults: &[&str], optional: bool) -> Result<String> {
    let value = env(variable, &find(defaults))?;
    if value.is_empty() && optional {
        return Ok(value);
    }
    if !executable(Path::new(&value)) {
        return Err(format!("{variable} does not select an executable file: {value}").into());
    }
    Ok(value)
}

pub(super) fn firefox(extra: &[String]) -> bool {
    extra.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--firefox-m10-proof"
                | "--firefox-m10-rendering-proof"
                | "--firefox-m10-dialog-proof"
                | "--firefox-m10-primary-proof"
                | "--firefox-m10-selection-proof"
                | "--firefox-m10-lifecycle-proof"
        )
    })
}

pub(super) fn positive(name: &str, default: &str) -> Result<String> {
    let value = env(name, default)?;
    if !value.starts_with(['1', '2', '3', '4', '5', '6', '7', '8', '9'])
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!("{name} must be a positive integer").into());
    }
    Ok(value)
}

pub(super) fn run(options: &BTreeMap<String, String>, extra: &[String]) -> Result<()> {
    let profile = required(options, "profile")?;
    let firefox = firefox(extra);
    let truecolor = env("SOPHIA_TRUECOLOR_PROOF", "false")? == "true";
    let optional = profile == "hagia" && !firefox && !truecolor;
    let mut terminal = String::new();
    let mut kind = String::new();
    let mut standalone = String::new();
    let mut browser = String::new();
    let mut benchmark = String::new();
    if profile == "standalone" {
        let workload = env("SOPHIA_STANDALONE_WORKLOAD", "vkcube")?;
        if !["vkcube", "kitty", "glxgears", "xterm"].contains(&workload.as_str()) {
            return Err("unknown standalone workload".into());
        }
        standalone = selected("SOPHIA_STANDALONE_APP_BIN", &[&workload], false)?;
        if workload == "glxgears" || workload == "xterm" {
            let prefix = if workload == "glxgears" {
                "SOPHIA_GLXGEARS"
            } else {
                "SOPHIA_XTERM"
            };
            let duration = positive(&format!("{prefix}_DURATION_SECONDS"), "20")?;
            let width = positive(&format!("{prefix}_WIDTH"), "500")?;
            let height = positive(&format!("{prefix}_HEIGHT"), "500")?;
            if workload == "glxgears" {
                benchmark = format!(
                    "sophia_glxgears_benchmark schema=1 duration_seconds={duration} surface_width={width} surface_height={height} swap_interval=1"
                );
            } else {
                let lines = positive("SOPHIA_XTERM_LINES", "1")?;
                let interval = positive("SOPHIA_XTERM_INTERVAL_MSEC", "16")?;
                if interval.parse::<u64>()? > 1000 {
                    return Err("SOPHIA_XTERM_INTERVAL_MSEC must be in 1..1000".into());
                }
                benchmark = format!(
                    "sophia_terminal_benchmark schema=2 workload=xterm-cpu duration_seconds={duration} surface_width={width} surface_height={height} lines_per_iteration={lines} interval_msec={interval}"
                );
            }
        } else if workload == "vkcube" {
            let direct = super::enabled("SOPHIA_ENABLE_DIRECT_SCANOUT")?;
            let frames = env(
                "SOPHIA_STANDALONE_FRAME_COUNT",
                if direct { "600" } else { "" },
            )?;
            if !frames.is_empty() {
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
                benchmark = format!(
                    "sophia_rendering_benchmark schema=1 workload=vkcube-xcb requested_frames={frames} surface_width={width} surface_height={height} vulkan_present_mode={mode}"
                );
            }
        }
    } else if ["hagia", "native", "kitty"].contains(&profile) {
        terminal = selected("SOPHIA_TERMINAL_BIN", &["kitty"], optional)?;
        if !optional {
            let resolved = std::fs::canonicalize(&terminal)?;
            kind = env(
                "SOPHIA_TERMINAL_KIND",
                resolved.file_name().and_then(|s| s.to_str()).unwrap_or(""),
            )?;
            if !["kitty", "xterm"].contains(&kind.as_str()) {
                return Err(format!("unsupported terminal kind: {kind}").into());
            }
            if (firefox || truecolor) && kind != "kitty" {
                return Err("proof profiles require the Kitty terminal adapter".into());
            }
        }
    } else {
        return Err("unknown launch profile".into());
    }
    if profile == "hagia" {
        browser = if firefox {
            selected("SOPHIA_FIREFOX_BIN", &["firefox"], false)?
        } else {
            selected("SOPHIA_HAGIA_BROWSER_BIN", &["helium", "firefox"], optional)?
        };
    }
    let wm = env("SOPHIA_HAGIA_BIN", &find(&["hagia"]))?;
    let mut output = std::io::stdout().lock();
    output.write_all(b"sophia_session_inputs schema=1 status=prepared\0")?;
    for value in [terminal, kind, browser, standalone, wm, benchmark] {
        output.write_all(value.as_bytes())?;
        output.write_all(b"\0")?;
    }
    Ok(())
}
