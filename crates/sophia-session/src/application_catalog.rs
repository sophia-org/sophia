mod desktop_entry;
mod execution;
pub use execution::*;
mod publication;
mod types;
mod worker;
pub use desktop_entry::desktop_exec_arguments;
pub use publication::PublishedApplicationCatalog;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::Path;
pub use types::*;
pub use worker::*;

fn read_entry(path: &Path) -> Result<Vec<u8>, String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::NOFOLLOW).bits() as i32)
        .open(path)
        .map_err(|_| "catalog file unavailable")?;
    if !file
        .metadata()
        .map_err(|_| "catalog metadata unavailable")?
        .is_file()
    {
        return Err("catalog entry is not a regular file".into());
    }
    if file
        .metadata()
        .map_err(|_| "catalog metadata unavailable")?
        .len()
        > MAX_DESKTOP_FILE_BYTES as u64
    {
        return Err("catalog file exceeds limit".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_DESKTOP_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "catalog read failed")?;
    if bytes.len() > MAX_DESKTOP_FILE_BYTES {
        return Err("catalog file exceeds limit".into());
    }
    Ok(bytes)
}

/// Runs on the catalog worker. Directory enumeration and parsing never run in
/// the session's input/render loop, and a partial scan is never published.
pub fn build_application_catalog(
    config: &sophia_config::ApplicationCatalogConfig,
    registered: &[RegisteredCatalogApplication],
    environment: &ApplicationCatalogEnvironment,
) -> Result<ApplicationCatalog, String> {
    let apps = registered
        .iter()
        .map(|a| (a.name.as_str(), &a.command))
        .collect::<BTreeMap<_, _>>();
    let terminal = config
        .terminal
        .as_ref()
        .map(|id| {
            apps.get(id.as_str())
                .copied()
                .ok_or("unknown catalog terminal")
        })
        .transpose()?;
    let mut catalog = ApplicationCatalog::default();
    let mut seen = BTreeSet::new();
    let mut total_bytes = 0;
    let mut scanned = 0;
    for name in &config.applications {
        let command = apps
            .get(name.as_str())
            .ok_or("unknown registered catalog application")?;
        catalog.entries.push(ApplicationCatalogEntry {
            descriptor: sophia_protocol::ShellApplicationDescriptor {
                slot: 0,
                label: desktop_entry::label(name, 128),
                keywords: String::new(),
                available: desktop_entry::executable(&command.executable),
            },
            command: Some((*command).clone()),
            source: None,
        });
    }
    for source in &config.sources {
        let root = match source.canonicalize() {
            Ok(p) => p,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("catalog source unavailable".into()),
        };
        let mut pending = vec![(root.clone(), 0)];
        let mut files = BTreeMap::new();
        while let Some((dir, depth)) = pending.pop() {
            for item in std::fs::read_dir(dir).map_err(|_| "catalog directory unavailable")? {
                scanned += 1;
                if scanned > MAX_CATALOG_SCAN_ENTRIES {
                    return Err("catalog scan exceeds limit".into());
                }
                let item = item.map_err(|_| "catalog enumeration failed")?;
                let kind = item.file_type().map_err(|_| "catalog entry unavailable")?;
                if kind.is_dir() {
                    if depth >= 8 {
                        return Err("catalog directory depth exceeds limit".into());
                    }
                    pending.push((item.path(), depth + 1));
                    continue;
                }
                let path = item.path();
                if path.extension().is_none_or(|e| e != "desktop") {
                    continue;
                }
                let id = path
                    .strip_prefix(&root)
                    .map_err(|_| "catalog source escaped")?
                    .to_str()
                    .ok_or("catalog filename is not UTF-8")?
                    .replace('/', "-");
                if files.insert(id, path).is_some() {
                    return Err("ambiguous desktop file identity".into());
                }
            }
        }
        for (id, path) in files {
            // A hidden or malformed higher-priority entry still masks a lower
            // source; falling through would undo the operator's source order.
            if !seen.insert(id) {
                continue;
            }
            let canonical = match path.canonicalize() {
                Ok(p) if p.starts_with(&root) => p,
                _ => {
                    catalog.skipped += 1;
                    continue;
                }
            };
            let bytes = match read_entry(&canonical) {
                Ok(b) => b,
                Err(_) => {
                    catalog.skipped += 1;
                    continue;
                }
            };
            total_bytes += bytes.len();
            if total_bytes > MAX_CATALOG_SOURCE_BYTES {
                return Err("catalog bytes exceed limit".into());
            }
            if let Some(entry) = desktop_entry::parse(
                &canonical,
                &bytes,
                environment,
                terminal,
                &config.terminal_arguments,
            ) {
                catalog.entries.push(entry);
            } else {
                catalog.skipped += 1;
            }
            if catalog.entries.len() > sophia_protocol::SOPHIA_SHELL_MAX_APPLICATIONS {
                return Err("application catalog exceeds limit".into());
            }
        }
    }
    for (index, entry) in catalog.entries.iter_mut().enumerate() {
        entry.descriptor.slot = (index + 1) as u16;
    }
    Ok(catalog)
}

/// A selection cannot silently acquire a replacement desktop command. The
/// immutable snapshot supplies argv; current bytes and executable availability
/// are checked again on the worker before the session dispatches the launch.
pub fn revalidate_catalog_entry(
    entry: &ApplicationCatalogEntry,
) -> Result<ApplicationLaunchCommand, String> {
    if let Some((path, bytes)) = &entry.source
        && read_entry(path)? != *bytes
    {
        return Err("desktop entry changed; reopen launcher".into());
    }
    let command = entry.command.clone().ok_or("application unavailable")?;
    if !desktop_entry::executable(&command.executable) {
        return Err("application executable unavailable".into());
    }
    Ok(command)
}
