//! Operator selections, not peer identities or effective capability grants.
use std::path::PathBuf;

use kdl::KdlNode;

use crate::{DesktopProfileError, ShellGpuMode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellComponentRole {
    Bar,
    ApplicationLauncher,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellComponentConfig {
    pub id: String,
    pub role: ShellComponentRole,
    pub executable: PathBuf,
    pub config: Option<PathBuf>,
    pub gpu: ShellGpuMode,
}

fn invalid(message: &str) -> DesktopProfileError {
    DesktopProfileError::Schema(format!("shell component: {message}"))
}

pub(crate) fn parse(node: &KdlNode) -> Result<ShellComponentConfig, DesktopProfileError> {
    if node.ty().is_some()
        || node.entries().len() != 2
        || node
            .entries()
            .iter()
            .any(|entry| entry.ty().is_some() || entry.name().is_some())
    {
        return Err(invalid("requires untyped positional identity and role"));
    }
    let id = node
        .get(0)
        .and_then(|value| value.as_string())
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .ok_or_else(|| {
            invalid("identity must be 1..64 ASCII letters, digits, hyphens or underscores")
        })?;
    let role = match node.get(1).and_then(|value| value.as_string()) {
        Some("bar") => ShellComponentRole::Bar,
        Some("application-launcher") => ShellComponentRole::ApplicationLauncher,
        _ => return Err(invalid("unsupported role")),
    };
    let children = node
        .children()
        .ok_or_else(|| invalid("requires executable settings"))?;
    let mut executable = None;
    let mut config = None;
    let mut gpu = None;
    for child in children.nodes() {
        match child.name().value() {
            "executable" if executable.is_none() => {
                executable = Some(
                    crate::session_candidate::component_arguments(child, 1)?
                        .remove(0)
                        .into(),
                );
            }
            "config" if config.is_none() => {
                config = Some(
                    crate::session_candidate::component_arguments(child, 1)?
                        .remove(0)
                        .into(),
                );
            }
            "gpu" if gpu.is_none() => {
                if child.ty().is_some()
                    || child.children().is_some()
                    || child.entries().len() != 1
                    || child.entries()[0].ty().is_some()
                    || child.entries()[0].name().is_some()
                {
                    return Err(invalid("gpu requires one untyped positional mode"));
                }
                gpu = Some(match child.get(0).and_then(|value| value.as_string()) {
                    Some("denied") => ShellGpuMode::Denied,
                    Some("direct") => ShellGpuMode::Direct,
                    _ => return Err(invalid("gpu must be denied or direct")),
                });
            }
            _ => return Err(invalid("unknown or repeated setting")),
        }
    }
    Ok(ShellComponentConfig {
        id: id.to_owned(),
        role,
        executable: executable.ok_or_else(|| invalid("executable is required"))?,
        config,
        gpu: gpu.unwrap_or_default(),
    })
}
