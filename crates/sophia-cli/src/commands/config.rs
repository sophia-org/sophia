use std::path::{Path, PathBuf};

mod session_preflight;

use sophia_config::{
    ConfigDomain, ConfigGeneration, ConfigSource, ConfigSourceClass,
    discover_default_config_source, load_core_snapshot, load_wm_snapshot,
};

pub(crate) fn try_run(args: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    if args.first().map(String::as_str) != Some("config") {
        return Ok(false);
    }
    let operation = args.get(1).map(String::as_str).unwrap_or("check");
    if operation == "check-session-profile" {
        session_preflight::run(&args[2..])?;
        return Ok(true);
    }
    if let Some(path) = desktop_profile_path(args)? {
        validate_desktop_profile_options(args)?;
        run_desktop_profile(operation, args, &path)?;
        return Ok(true);
    }
    let wm = args.iter().skip(2).any(|argument| argument == "--wm");
    let domain = if wm {
        ConfigDomain::Wm
    } else {
        ConfigDomain::Core
    };
    let explicit = explicit_path(args, wm)?;
    validate_options(args, wm)?;
    let source = discover_default_config_source(domain, explicit.as_deref());
    match operation {
        "check" => check(domain, &source)?,
        "print-effective" => print_effective(domain, &source)?,
        other => {
            return Err(format!(
                "unknown config operation {other:?}; expected check or print-effective"
            )
            .into());
        }
    }
    Ok(true)
}

fn desktop_profile_path(args: &[String]) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let values = args
        .iter()
        .skip(2)
        .filter_map(|argument| argument.strip_prefix("--desktop-profile="))
        .collect::<Vec<_>>();
    if values.len() > 1 {
        return Err("duplicate --desktop-profile".into());
    }
    let path = values.first().map(PathBuf::from);
    if path
        .as_ref()
        .is_some_and(|path| !path.is_absolute() || path.as_os_str().is_empty())
    {
        return Err("--desktop-profile requires an absolute path".into());
    }
    Ok(path)
}

fn validate_desktop_profile_options(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let components = args
        .iter()
        .filter(|arg| arg.starts_with("--component="))
        .count();
    if components > 1
        || (components > 0 && args.get(1).map(String::as_str) != Some("print-component"))
    {
        return Err("--component is allowed once, with print-component".into());
    }
    for argument in args.iter().skip(2) {
        if argument == "-v"
            || argument == "--verbose"
            || argument.starts_with("--desktop-profile=")
            || argument.starts_with("--component=")
        {
            continue;
        }
        return Err(format!("desktop profile check rejects option {argument:?}").into());
    }
    Ok(())
}

fn run_desktop_profile(
    operation: &str,
    args: &[String],
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let prepared =
        sophia_config::load_prepared_desktop_profile(Some(path), ConfigGeneration::INITIAL)?;
    let profile = &prepared.profile;
    match operation {
        "check" => println!(
            "valid domain=desktop-profile schema=1 generation={} digest={} sources={} policy_validation=delegated",
            profile.generation.raw(),
            profile.digest,
            profile.sources.len()
        ),
        "print-effective" | "print-policy" => {
            if operation == "print-effective" {
                println!("// Source: {path:?}");
                println!(
                    "// Profile selections before launcher overrides; omitted components inherit defaults."
                );
                print!("{}", sophia_config::render_desktop_profile_source(profile)?);
                return Ok(());
            }
            println!("schema 1");
            for authority in sophia_config::DesktopAuthority::ALL {
                if operation == "print-policy"
                    && authority != sophia_config::DesktopAuthority::Policy
                {
                    continue;
                }
                println!("{} {{", authority.name());
                for value in &profile.candidates[&authority].values {
                    println!("    {}", value.encoded);
                }
                println!("}}");
            }
        }
        "print-component" => {
            let selected = match args.iter().find_map(|arg| arg.strip_prefix("--component=")) {
                Some("window-manager") => {
                    if let Some(wm) = prepared.candidates.session.components.window_manager {
                        Some(wm.executable)
                    } else {
                        let source = discover_default_config_source(ConfigDomain::Core, None);
                        load_core_snapshot(&source, ConfigGeneration::INITIAL)?
                            .external_wm
                            .map(|wm| wm.executable)
                    }
                }
                Some("shell-client") => prepared.candidates.session.components.shell_client,
                _ => {
                    return Err(
                        "print-component requires --component=window-manager|shell-client".into(),
                    );
                }
            };
            if let Some(path) = selected {
                println!("{}", path.display());
            }
        }
        _ => return Err(
            "desktop profiles support check, print-effective, print-policy, and print-component"
                .into(),
        ),
    }
    Ok(())
}

fn validate_options(args: &[String], wm: bool) -> Result<(), Box<dyn std::error::Error>> {
    for argument in args.iter().skip(2) {
        let valid_path = if wm {
            argument.starts_with("--wm-config=")
        } else {
            argument.starts_with("--config=")
        };
        if argument == "--wm" || argument == "-v" || argument == "--verbose" || valid_path {
            continue;
        }
        return Err(format!("unknown config option {argument:?}").into());
    }
    Ok(())
}

fn explicit_path(args: &[String], wm: bool) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let option = if wm { "--wm-config" } else { "--config" };
    let incompatible = if wm { "--config=" } else { "--wm-config=" };
    if args
        .iter()
        .skip(2)
        .any(|argument| argument.starts_with(incompatible))
    {
        return Err(format!("{incompatible} does not select this configuration domain").into());
    }
    let values = args
        .iter()
        .skip(2)
        .filter_map(|argument| argument.strip_prefix(&format!("{option}=")))
        .collect::<Vec<_>>();
    if values.len() > 1 {
        return Err(format!("duplicate {option}").into());
    }
    let path = values.first().map(PathBuf::from);
    if path
        .as_ref()
        .is_some_and(|path| !path.is_absolute() || path.as_os_str().is_empty())
    {
        return Err(format!("{option} requires an absolute path").into());
    }
    Ok(path)
}

fn check(domain: ConfigDomain, source: &ConfigSource) -> Result<(), Box<dyn std::error::Error>> {
    let (schema, digest) = match domain {
        ConfigDomain::Core => {
            let snapshot = load_core_snapshot(source, ConfigGeneration::INITIAL)?;
            (snapshot.schema, snapshot.digest)
        }
        ConfigDomain::Wm => {
            let snapshot = load_wm_snapshot(source, ConfigGeneration::INITIAL)?;
            (snapshot.schema, snapshot.digest)
        }
    };
    println!(
        "valid domain={} schema={} digest={} source={}",
        domain_name(domain),
        schema,
        digest,
        source_name(source)
    );
    Ok(())
}

fn print_effective(
    domain: ConfigDomain,
    source: &ConfigSource,
) -> Result<(), Box<dyn std::error::Error>> {
    match domain {
        ConfigDomain::Core => {
            let snapshot = load_core_snapshot(source, ConfigGeneration::INITIAL)?;
            println!("{snapshot:#?}");
        }
        ConfigDomain::Wm => {
            let snapshot = load_wm_snapshot(source, ConfigGeneration::INITIAL)?;
            println!("{snapshot:#?}");
        }
    }
    Ok(())
}

fn source_name(source: &ConfigSource) -> String {
    match (&source.class, &source.path) {
        (ConfigSourceClass::CompiledDefault, _) => "compiled-default".to_owned(),
        (_, Some(path)) => path.display().to_string(),
        (_, None) => "invalid-source".to_owned(),
    }
}

const fn domain_name(domain: ConfigDomain) -> &'static str {
    match domain {
        ConfigDomain::Core => "core",
        ConfigDomain::Wm => "wm",
    }
}
