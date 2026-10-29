use std::path::{Component, PathBuf};

pub(super) fn capture_delivery_directory(
    args: &[String],
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    const KEY: &str = "--diagnostic-capture-directory";
    let flags: Vec<_> = args
        .iter()
        .filter(|arg| arg.split('=').next() == Some(KEY))
        .collect();
    if flags.is_empty() {
        return Ok(None);
    }
    if !cfg!(feature = "diagnostic-capture-delivery") {
        return Err("requires a build with diagnostic-capture-delivery".into());
    }
    if flags.len() != 1 {
        return Err("capture directory must occur exactly once".into());
    }
    let path = flags[0]
        .strip_prefix("--diagnostic-capture-directory=")
        .ok_or("capture directory requires =PATH")?;
    let path = PathBuf::from(path);
    if !path.is_absolute()
        || path.components().any(|c| matches!(c, Component::ParentDir))
        || path
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .any(|b| b.is_ascii_control())
    {
        return Err(
            "capture directory must be an absolute path without traversal or controls".into(),
        );
    }
    Ok(Some(path))
}
