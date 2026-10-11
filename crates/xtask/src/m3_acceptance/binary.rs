//! Acceptance runs Cargo artifacts in place; evidence retains their identity.
use super::{identity, types::Report};
use std::path::{Component, Path, PathBuf};

pub(super) fn resolve(target: &Path, relative: &Path) -> Result<PathBuf, String> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("test binary requires a normal target-relative path".into());
    }
    let path = target.join(relative);
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    let root = target.canonicalize().map_err(|error| error.to_string())?;
    let parent = path
        .parent()
        .ok_or("test binary has no parent")?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() || !parent.starts_with(root) {
        return Err("test binary must be a regular file inside the build target".into());
    }
    Ok(path)
}

pub(super) fn validate(report: &Report, target: &Path) -> Result<(), String> {
    let binary = report
        .binary
        .as_ref()
        .ok_or("test binary evidence absent")?;
    if binary.get("path").is_some() {
        return Err("copied binary evidence cannot attest a target artifact".into());
    }
    let relative = binary["target_path"]
        .as_str()
        .ok_or("test binary target path absent")?;
    let path = resolve(target, Path::new(relative))?;
    if binary["sha256"].as_str() != Some(&identity::digest(&path)?) {
        return Err("test binary identity changed after contained execution".into());
    }
    Ok(())
}
