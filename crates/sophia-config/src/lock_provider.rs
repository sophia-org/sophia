//! The session lock provider selection (t294): at most one renderer for
//! what a locked session shows.
//!
//! ```kdl
//! session {
//!     lock-provider {
//!         executable "/usr/libexec/kleis"
//!         config "/home/user/.config/kleis/config.kdl"
//!         gpu "denied"
//!     }
//! }
//! ```
//!
//! A provider holds no authority over the lock and never sees the secret;
//! without one, a locked session shows Engine's fill and unlocks the same.
use std::path::PathBuf;

use kdl::KdlNode;

use crate::{DesktopProfileError, ShellGpuMode};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockProviderConfig {
    pub executable: PathBuf,
    pub config: Option<PathBuf>,
    /// The same grant as shell components: denied unless chosen.
    pub gpu: ShellGpuMode,
}

fn invalid(message: &str) -> DesktopProfileError {
    DesktopProfileError::Schema(format!("session lock-provider: {message}"))
}

fn one_string<'a>(node: &'a KdlNode, setting: &str) -> Result<&'a str, DesktopProfileError> {
    if node.ty().is_some()
        || node.children().is_some()
        || node.entries().len() != 1
        || node.entries()[0].ty().is_some()
        || node.entries()[0].name().is_some()
    {
        return Err(invalid(&format!("{setting} requires one untyped value")));
    }
    node.get(0)
        .and_then(|value| value.as_string())
        .ok_or_else(|| invalid(&format!("{setting} must be a string")))
}

fn absolute(node: &KdlNode, setting: &str) -> Result<PathBuf, DesktopProfileError> {
    let path = PathBuf::from(one_string(node, setting)?);
    if !path.is_absolute() {
        return Err(invalid(&format!("{setting} must be an absolute path")));
    }
    Ok(path)
}

pub(crate) fn parse(node: &KdlNode) -> Result<LockProviderConfig, DesktopProfileError> {
    if node.ty().is_some() || !node.entries().is_empty() {
        return Err(invalid("takes no arguments, only settings"));
    }
    let children = node
        .children()
        .ok_or_else(|| invalid("requires an executable"))?;
    let mut executable = None;
    let mut config = None;
    let mut gpu = None;
    for child in children.nodes() {
        match child.name().value() {
            "executable" if executable.is_none() => {
                executable = Some(absolute(child, "executable")?);
            }
            "config" if config.is_none() => config = Some(absolute(child, "config")?),
            "gpu" if gpu.is_none() => {
                gpu = Some(match one_string(child, "gpu")? {
                    "denied" => ShellGpuMode::Denied,
                    "direct" => ShellGpuMode::Direct,
                    _ => return Err(invalid("gpu must be denied or direct")),
                });
            }
            _ => return Err(invalid("unknown or repeated setting")),
        }
    }
    Ok(LockProviderConfig {
        executable: executable.ok_or_else(|| invalid("executable is required"))?,
        config,
        gpu: gpu.unwrap_or_default(),
    })
}
