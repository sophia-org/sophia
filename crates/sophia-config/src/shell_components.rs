//! Operator selections, not peer identities or effective capability grants.
use std::path::PathBuf;

use kdl::KdlNode;

use crate::{DesktopProfileError, ShellGpuMode};

/// Maximum independently selected component owners in one Session.
pub const MAX_SHELL_COMPONENTS: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellComponentRole {
    Bar,
    ApplicationLauncher,
    Dock,
}

/// Logical output edge reserved by an operator-selected persistent component.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellComponentEdge {
    Top,
    Bottom,
    Left,
    Right,
}
impl ShellComponentEdge {
    pub const fn wire(self) -> u16 {
        match self {
            Self::Top => 1,
            Self::Bottom => 3,
            Self::Left => 4,
            Self::Right => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellComponentReservation {
    pub edge: ShellComponentEdge,
    pub max_thickness: u16,
}

/// The wire one component's connection uses, fixed at startup. Current IPC
/// remains the default; a selection never changes the role's grants.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ShellTransportSelection {
    #[default]
    CurrentIpc,
    /// `sophia_shell_fs_v1` over 9P2000.L.
    NineP2000L,
}

impl ShellTransportSelection {
    /// The KDL value, matching the WM's `--wm-transport` names.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::CurrentIpc => "current-ipc",
            Self::NineP2000L => "9p2000.L",
        }
    }

    /// The environment variable that names the component's endpoint, so a
    /// client can never mistake one wire's socket for the other's.
    pub const fn socket_env(self) -> &'static str {
        match self {
            Self::CurrentIpc => "SOPHIA_SHELL_SOCKET",
            Self::NineP2000L => "SOPHIA_SHELL_9P_SOCKET",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellComponentConfig {
    pub id: String,
    pub role: ShellComponentRole,
    pub executable: PathBuf,
    pub config: Option<PathBuf>,
    pub gpu: ShellGpuMode,
    pub reservation: Option<ShellComponentReservation>,
    pub transport: ShellTransportSelection,
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
        Some("dock") => ShellComponentRole::Dock,
        _ => return Err(invalid("unsupported role")),
    };
    let children = node
        .children()
        .ok_or_else(|| invalid("requires executable settings"))?;
    let mut executable = None;
    let mut config = None;
    let mut gpu = None;
    let mut reservation = None;
    let mut transport = None;
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
            "transport" if transport.is_none() => {
                if child.ty().is_some()
                    || child.children().is_some()
                    || child.entries().len() != 1
                    || child.entries()[0].ty().is_some()
                    || child.entries()[0].name().is_some()
                {
                    return Err(invalid("transport requires one untyped positional wire"));
                }
                transport = Some(match child.get(0).and_then(|value| value.as_string()) {
                    Some("current-ipc") => ShellTransportSelection::CurrentIpc,
                    Some("9p2000.L") => ShellTransportSelection::NineP2000L,
                    _ => return Err(invalid("transport must be current-ipc or 9p2000.L")),
                });
            }
            "reservation" if reservation.is_none() => {
                reservation = Some(parse_reservation(child)?);
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
    if role == ShellComponentRole::ApplicationLauncher && reservation.is_some() {
        return Err(invalid(
            "transient launcher cannot reserve a persistent edge",
        ));
    }
    Ok(ShellComponentConfig {
        id: id.to_owned(),
        role,
        executable: executable.ok_or_else(|| invalid("executable is required"))?,
        config,
        gpu: gpu.unwrap_or_default(),
        reservation,
        transport: transport.unwrap_or_default(),
    })
}

fn parse_reservation(node: &KdlNode) -> Result<ShellComponentReservation, DesktopProfileError> {
    if node.ty().is_some()
        || node.children().is_some()
        || node.entries().len() != 2
        || node
            .entries()
            .iter()
            .any(|entry| entry.ty().is_some() || entry.name().is_some())
    {
        return Err(invalid("reservation requires an edge and a thickness"));
    }
    let edge = match node.get(0).and_then(|v| v.as_string()) {
        Some("top") => ShellComponentEdge::Top,
        Some("bottom") => ShellComponentEdge::Bottom,
        Some("left") => ShellComponentEdge::Left,
        Some("right") => ShellComponentEdge::Right,
        _ => return Err(invalid("unsupported reservation edge")),
    };
    let max_thickness = node
        .get(1)
        .and_then(|v| v.as_integer())
        .and_then(|v| u16::try_from(v).ok())
        .filter(|v| (1..=512).contains(v))
        .ok_or_else(|| invalid("reservation thickness must be in 1..512"))?;
    Ok(ShellComponentReservation {
        edge,
        max_thickness,
    })
}

/// Validate the complete selected inventory before endpoints or resources exist.
pub fn validate_shell_component_reservations(
    components: &[ShellComponentConfig],
) -> Result<(), DesktopProfileError> {
    let dock = components
        .iter()
        .any(|c| c.role == ShellComponentRole::Dock);
    let mut occupied = [false; 4];
    for component in components {
        if component.role == ShellComponentRole::ApplicationLauncher {
            if component.reservation.is_some() {
                return Err(invalid(
                    "transient launcher cannot reserve a persistent edge",
                ));
            }
            continue;
        }
        if dock && component.reservation.is_none() {
            return Err(invalid(
                "dock coexistence requires explicit persistent edge reservations",
            ));
        }
        if let Some(reservation) = component.reservation {
            if !(1..=512).contains(&reservation.max_thickness) {
                return Err(invalid("reservation thickness must be in 1..512"));
            }
            let slot = usize::from(reservation.edge.wire() - 1);
            if occupied[slot] {
                return Err(invalid("persistent component edges conflict"));
            }
            occupied[slot] = true;
        }
    }
    Ok(())
}
