//! Validation shared by generic launch acceptance and launch controls.
//! These checks stay with Sophia when product discovery and proof staging move.
use super::{BTreeMap, Result, env, required};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
};

pub(super) fn positive(name: &str, default: &str) -> Result<String> {
    let value = env(name, default)?;
    if !value.starts_with(['1', '2', '3', '4', '5', '6', '7', '8', '9'])
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!("{name} must be a positive integer").into());
    }
    Ok(value)
}

pub(super) fn private_state(options: &BTreeMap<String, String>) -> Result<PathBuf> {
    let path = PathBuf::from(required(options, "state-dir")?);
    let meta = fs::symlink_metadata(&path)?;
    if !path.is_absolute()
        || !meta.is_dir()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.permissions().mode() & 0o077 != 0
    {
        return Err("state-dir must be an absolute private owner directory".into());
    }
    Ok(path)
}
