use super::types::*;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

pub(super) fn executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
pub(super) fn resolve(program: &str, env: &ApplicationCatalogEnvironment) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() {
        return executable(path).then(|| path.to_owned());
    }
    if program.contains('/') || program.is_empty() {
        return None;
    }
    env.search_path
        .iter()
        .filter(|p| p.is_absolute())
        .map(|p| p.join(program))
        .find(|p| executable(p))
}
fn unescape(value: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(match chars.next()? {
                's' => ' ',
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\\' => '\\',
                _ => return None,
            });
        } else {
            out.push(c);
        }
    }
    Some(out)
}
fn localized(fields: &BTreeMap<String, String>, key: &str, locale: &str) -> Option<String> {
    let (base_locale, suffix) = locale
        .split_once('@')
        .map_or((locale, None), |(base, modifier)| (base, Some(modifier)));
    let locale = format!(
        "{}{}",
        base_locale.split('.').next().unwrap_or(base_locale),
        suffix.map_or(String::new(), |m| format!("@{m}"))
    );
    let locale = locale.as_str();
    let base = locale.split('@').next().unwrap_or(locale);
    let language = base.split('_').next().unwrap_or(base);
    let modifier = locale
        .split_once('@')
        .map(|(_, m)| format!("{language}@{m}"));
    for variant in [
        Some(locale),
        Some(base),
        modifier.as_deref(),
        Some(language),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(v) = fields.get(&format!("{key}[{variant}]")) {
            return unescape(v);
        }
    }
    fields.get(key).and_then(|v| unescape(v))
}
pub(super) fn label(value: &str, max: usize) -> String {
    let mut out = String::new();
    for c in value
        .chars()
        .filter(|c| !c.is_control() && !matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}'))
    {
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    out.trim().to_owned()
}

/// Desktop Exec has its own quoting rules. It is never shell source.
pub fn desktop_exec_arguments(
    exec: &str,
    name: &str,
    icon: Option<&str>,
    path: &Path,
) -> Option<Vec<String>> {
    let decoded = unescape(exec)?;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = decoded.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => {
                let c = chars.next()?;
                if !matches!(c, '"' | '`' | '$' | '\\') {
                    return None;
                }
                word.push(c);
                started = true;
            }
            ' ' if !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            '%' if quoted => return None,
            c if c.is_control() => return None,
            c if !quoted
                && matches!(
                    c,
                    '\'' | '\\'
                        | '>'
                        | '<'
                        | '~'
                        | '|'
                        | '&'
                        | ';'
                        | '$'
                        | '*'
                        | '?'
                        | '#'
                        | '('
                        | ')'
                        | '`'
                ) =>
            {
                return None;
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if started {
        words.push(word);
    }
    let mut args = Vec::new();
    let mut file_fields = 0;
    for word in words {
        if matches!(word.as_str(), "%f" | "%F" | "%u" | "%U") {
            file_fields += 1;
            continue;
        }
        if word == "%i" {
            if let Some(icon) = icon.filter(|v| !v.is_empty()) {
                args.push("--icon".into());
                args.push(icon.into());
            }
            continue;
        }
        let mut value = String::new();
        let mut chars = word.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                value.push(c);
                continue;
            }
            match chars.next()? {
                '%' => value.push('%'),
                'c' => value.push_str(name),
                'k' => value.push_str(path.to_str()?),
                'd' | 'D' | 'n' | 'N' | 'v' | 'm' => {}
                _ => return None,
            }
        }
        if !value.is_empty() || word.is_empty() {
            args.push(value);
        }
    }
    if file_fields > 1
        || args.is_empty()
        || args.len() > 64
        || args.iter().any(|a| a.len() > 4096 || a.contains('\0'))
        || args[0].is_empty()
        || args[0].contains('=')
    {
        return None;
    }
    Some(args)
}

pub(super) fn parse(
    path: &Path,
    bytes: &[u8],
    env: &ApplicationCatalogEnvironment,
    terminal: Option<&ApplicationLaunchCommand>,
    terminal_args: &[String],
) -> Option<ApplicationCatalogEntry> {
    let source = std::str::from_utf8(bytes).ok()?;
    let mut fields = BTreeMap::new();
    let mut active = false;
    let mut seen = false;
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            active = line == "[Desktop Entry]";
            if active && seen {
                return None;
            }
            seen |= active;
            continue;
        }
        if active {
            let (k, v) = line.split_once('=')?;
            if fields.insert(k.trim().to_owned(), v.to_owned()).is_some() {
                return None;
            }
        }
    }
    if fields.get("Type").map(String::as_str) != Some("Application")
        || fields.get("Hidden").is_some_and(|v| v == "true")
        || fields.get("NoDisplay").is_some_and(|v| v == "true")
    {
        return None;
    }
    let includes = |key: &str| {
        fields.get(key).is_some_and(|v| {
            v.split(';')
                .any(|d| env.current_desktop.iter().any(|e| e == d))
        })
    };
    if fields.contains_key("OnlyShowIn") && !includes("OnlyShowIn") || includes("NotShowIn") {
        return None;
    }
    if let Some(program) = fields.get("TryExec") {
        resolve(&unescape(program)?, env)?;
    }
    let name = localized(&fields, "Name", &env.locale)?;
    let display = label(&name, 128);
    if display.is_empty() {
        return None;
    }
    let keywords = label(
        &format!(
            "{} {}",
            localized(&fields, "GenericName", &env.locale).unwrap_or_default(),
            localized(&fields, "Keywords", &env.locale).unwrap_or_default()
        ),
        256,
    );
    let command = (|| {
        if fields.get("DBusActivatable").is_some_and(|v| v != "false")
            || fields
                .get("Terminal")
                .is_some_and(|v| !matches!(v.as_str(), "true" | "false"))
        {
            return None;
        }
        let icon = fields.get("Icon").and_then(|v| unescape(v));
        let argv = desktop_exec_arguments(fields.get("Exec")?, &name, icon.as_deref(), path)?;
        let executable = resolve(&argv[0], env)?;
        let working_directory = match fields.get("Path").filter(|v| !v.is_empty()) {
            Some(value) => Some(PathBuf::from(unescape(value)?)),
            None => None,
        };
        if working_directory
            .as_ref()
            .is_some_and(|p| !p.is_absolute() || !p.is_dir())
        {
            return None;
        }
        let mut command = ApplicationLaunchCommand {
            executable,
            arguments: argv[1..].to_vec(),
            working_directory,
        };
        if fields.get("Terminal").is_some_and(|v| v == "true") {
            let terminal = terminal?;
            let mut args = terminal.arguments.clone();
            args.extend_from_slice(terminal_args);
            args.push(command.executable.to_str()?.to_owned());
            args.extend(command.arguments);
            command = ApplicationLaunchCommand {
                executable: terminal.executable.clone(),
                arguments: args,
                working_directory: command.working_directory,
            };
        }
        Some(command)
    })();
    Some(ApplicationCatalogEntry {
        identity: String::new(), // Assigned from the source-relative desktop ID by the scanner.
        descriptor: sophia_protocol::ShellApplicationDescriptor {
            slot: 0,
            available: command.is_some(),
            label: display,
            keywords,
        },
        command,
        source: Some((path.to_owned(), bytes.to_vec())),
    })
}
