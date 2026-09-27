//! Product-neutral launch controls, host checks and exact argument validation.
use std::{collections::BTreeMap, io::Write};

mod acceptance;
mod bounded;
mod controls;
mod environment;
mod host;
mod validation;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn try_run(args: &[String]) -> Result<bool> {
    if args.first().map(String::as_str) != Some("session") {
        return Ok(false);
    }
    let Some(command) = args.get(1).map(String::as_str) else {
        return Ok(false);
    };
    if !matches!(
        command,
        "check-host" | "check-launch" | "prepare-controls" | "prepare-environment"
    ) {
        return Ok(false);
    }
    let (options, extra) = parse(&args[2..])?;
    match command {
        "check-host" => host::run(&options, extra)?,
        "check-launch" => acceptance::run(&options, extra)?,
        "prepare-controls" => controls::run(&options, extra)?,
        "prepare-environment" => environment::run(&options, extra)?,
        _ => unreachable!(),
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
        if !["profile", "state-dir", "tty", "allow-active"].contains(&key)
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
