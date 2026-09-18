//! Installed diagnostics presentation. Persistence and lifecycle live in Session.
use std::path::Path;

use sophia_session::diagnostics::{Store, executable_digest};

type Error = Box<dyn std::error::Error>;

pub(super) fn try_run(args: &[String]) -> Result<bool, Error> {
    if args.first().map(String::as_str) != Some("session") {
        return Ok(false);
    }
    let Some(command) = args.get(1).map(String::as_str) else {
        return Ok(false);
    };
    if !matches!(
        command,
        "mark" | "inspect" | "keep" | "list" | "launches" | "stderr" | "_supervise"
    ) {
        return Ok(false);
    }
    let tail = &args[2..];
    if command == "_supervise" {
        let profile = tail
            .first()
            .and_then(|arg| arg.strip_prefix("--profile="))
            .ok_or("expected --profile=NAME")?;
        if tail.get(1).map(String::as_str) != Some("--") || tail.len() < 3 {
            return Err("expected -- COMMAND [ARGS]".into());
        }
        let identity = launch_identity()?;
        let status = sophia_session::diagnostics::supervise(
            profile,
            &identity,
            Path::new(&tail[2]),
            &tail[3..],
        )?;
        std::process::exit(status);
    }
    let store = Store::from_environment()?;
    match command {
        "mark" => {
            let mut selector = None;
            let mut label = None;
            for argument in tail {
                if let Some(value) = argument.strip_prefix("--session=") {
                    if selector.replace(value).is_some() || value.is_empty() {
                        return Err("specify one --session=ID".into());
                    }
                } else if argument.starts_with('-') || label.replace(argument.as_str()).is_some() {
                    return Err("usage: sophia session mark [--session=ID|latest] [LABEL]".into());
                }
            }
            let marker = store.mark(selector, label.unwrap_or(""))?;
            println!(
                "session={} marker={}\nevidence={}\ninspect: sophia session inspect {} --marker={}\npreserve: sophia session keep {}",
                marker.session,
                marker.id,
                marker.path.display(),
                marker.session,
                marker.id,
                marker.session
            );
        }
        "inspect" => {
            if tail.is_empty() || tail.len() > 2 {
                return Err("usage: sophia session inspect ID|latest [--marker=ID]".into());
            }
            let marker = tail
                .get(1)
                .map(|arg| arg.strip_prefix("--marker=").ok_or("expected --marker=ID"))
                .transpose()?;
            let result = store.inspect(&tail[0], marker)?;
            println!(
                "session={} status={} evidence={} bytes={}",
                result.record.id,
                result.record.status,
                result.record.path.display(),
                result.record.bytes
            );
            // Process identity is needed to resolve a stale owner, not part of
            // the public diagnostic summary or an Engine/client protocol.
            for line in result
                .manifest
                .lines()
                .filter(|line| !line.starts_with("owner_"))
            {
                println!("{line}");
            }
            print!("{}{}{}", result.identities, result.health, result.lifecycle);
            println!(
                "retained_boot_msec={:?}..{:?} requested_boot_msec={:?}..{:?}",
                result.retained_first_msec,
                result.retained_last_msec,
                result.window_start_msec,
                result.window_end_msec
            );
            println!(
                "Unobserved intervals, discarded records, and rotated history are not proof of inactivity."
            );
            for event in result.events {
                println!("{event}");
            }
            print!("{}", result.markers);
        }
        "keep" => {
            if tail.is_empty()
                || tail.len() > 2
                || tail
                    .get(1)
                    .is_some_and(|arg| arg != "--include-application-stderr")
            {
                return Err(
                    "usage: sophia session keep ID|latest [--include-application-stderr]".into(),
                );
            }
            println!(
                "preserved={}",
                store
                    .keep_with_application_stderr(&tail[0], tail.len() == 2)?
                    .display()
            );
        }
        "launches" => {
            if tail.len() != 1 {
                return Err("usage: sophia session launches ID|latest".into());
            }
            let records = store.application_records(&tail[0])?;
            print!("{}", records.health);
            for launch in records.launches.values() {
                print!("{launch}");
            }
            println!(
                "incomplete_tail={} rotated or dropped records may be absent; queued bytes are not persistence acknowledgements",
                records.incomplete_tail
            );
        }
        "stderr" => {
            use std::io::Write;
            if !(2..=3).contains(&tail.len()) || tail.get(2).is_some_and(|arg| arg != "--raw") {
                return Err("usage: sophia session stderr ID|latest --launch=ID [--raw]".into());
            }
            let id: u64 = tail[1]
                .strip_prefix("--launch=")
                .ok_or("expected --launch=ID")?
                .parse()?;
            if id == 0 {
                return Err("launch identity must be nonzero".into());
            }
            let records = store.application_records(&tail[0])?;
            let mut found = false;
            let mut next = 0;
            for (launch, offset, bytes) in records.chunks {
                if launch != id {
                    continue;
                }
                found = true;
                if offset != next {
                    eprintln!("stderr unavailable range: {next}..{offset}");
                }
                next = offset + bytes.len() as u64;
                if tail.len() == 3 {
                    std::io::stdout().lock().write_all(&bytes)?;
                } else {
                    println!(
                        "{offset}: {}",
                        sophia_session::diagnostics::application::escape_bytes(&bytes)
                    );
                }
            }
            if !found {
                eprintln!("no retained stderr bytes for launch {id}");
            }
            eprintln!(
                "Retained bytes only; rotation, truncation, live collection, or an incomplete tail may omit output."
            );
        }
        "list" => {
            if !tail.is_empty() {
                return Err("usage: sophia session list".into());
            }
            let records = store.list()?;
            let total = records.iter().map(|r| r.bytes).sum::<u64>();
            for record in records {
                println!(
                    "session={} profile={} status={} recording={} bytes={} evidence={}",
                    record.id,
                    record.profile,
                    record.status,
                    record.recording,
                    record.bytes,
                    record.path.display()
                );
            }
            println!(
                "rolling_bytes={total} preserved_bytes={}",
                store.preserved_bytes()?
            );
        }

        _ => unreachable!(),
    }
    Ok(true)
}

pub(crate) fn launch_identity() -> Result<String, Error> {
    let mut result = format!(
        "sophia_binary_sha256={}\n",
        executable_digest(&std::env::current_exe()?)?
    );
    for (key, variable) in [
        ("release_version", "SOPHIA_INSTALLED_VERSION"),
        ("release_commit", "SOPHIA_INSTALLED_COMMIT"),
        ("profile_root_sha256", "SOPHIA_DESKTOP_PROFILE_SHA256"),
    ] {
        let value = std::env::var(variable)
            .ok()
            .filter(|value| {
                value.len() <= 128
                    && value
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
            })
            .unwrap_or_else(|| "unavailable".into());
        result.push_str(&format!("{key}={value}\n"));
    }
    result.push_str("component_private_configuration=not_observed\n");
    Ok(result)
}
