use kdl::{KdlDocument, KdlNode};

use crate::{
    ConfigDigest, ConfigGeneration, DesktopAuthority, DesktopAuthorityCandidate,
    DesktopProfileError,
};

pub const DESKTOP_SESSION_MAX_APPLICATION_NAME_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DesktopControlAccess {
    #[default]
    Disabled,
    HostAdmin,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DesktopInspectionAccess {
    #[default]
    Disabled,
    HostAdmin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopSessionCandidate {
    pub generation: ConfigGeneration,
    pub digest: ConfigDigest,
    pub terminal: Option<String>,
    pub browser: Option<String>,
    pub application_catalog: Option<String>,
    pub startup: Option<Vec<String>>,
    pub logout_enabled: Option<bool>,
    pub control: DesktopControlAccess,
    pub inspection: DesktopInspectionAccess,
    pub components: crate::DesktopComponents,
    pub applications: Vec<crate::DesktopApplication>,
}

fn schema_error(message: impl Into<String>) -> DesktopProfileError {
    DesktopProfileError::Schema(format!("session candidate: {}", message.into()))
}

fn single_node(encoded: &str) -> Result<KdlNode, DesktopProfileError> {
    let document = KdlDocument::parse_v2(encoded)
        .map_err(|error| schema_error(format!("invalid staged value: {error}")))?;
    if document.nodes().len() != 1 {
        return Err(schema_error("staged value must contain exactly one node"));
    }
    Ok(document.nodes()[0].clone())
}

fn application_names(
    node: &KdlNode,
    setting: &str,
    maximum: usize,
) -> Result<Vec<String>, DesktopProfileError> {
    if (node.entries().is_empty() && setting != "startup")
        || node.entries().len() > maximum
        || node.children().is_some()
        || node.ty().is_some()
    {
        return Err(schema_error(format!(
            "{setting} requires at most {maximum} application identities"
        )));
    }
    let mut names = Vec::new();
    for entry in node.entries() {
        if entry.name().is_some() || entry.ty().is_some() {
            return Err(schema_error(format!(
                "{setting} requires untyped positional identities"
            )));
        }
        let name = entry
            .value()
            .as_string()
            .filter(|name| {
                !name.is_empty() && name.len() <= DESKTOP_SESSION_MAX_APPLICATION_NAME_BYTES
            })
            .ok_or_else(|| schema_error(format!("{setting} application identity is invalid")))?;
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(schema_error(format!(
                "{setting} application identity contains unsupported characters"
            )));
        }
        if names.iter().any(|previous| previous == name) {
            return Err(schema_error(format!(
                "{setting} repeats an application identity"
            )));
        }
        names.push(name.to_owned());
    }
    Ok(names)
}

fn application_name(node: &KdlNode, setting: &str) -> Result<String, DesktopProfileError> {
    Ok(application_names(node, setting, 1)?.remove(0))
}

fn logout_enabled(node: &KdlNode) -> Result<bool, DesktopProfileError> {
    if node.entries().len() != 1 || node.children().is_some() || node.ty().is_some() {
        return Err(schema_error("logout requires one boolean"));
    }
    node.get(0)
        .and_then(|value| value.as_bool())
        .ok_or_else(|| schema_error("logout requires one boolean"))
}

pub(crate) fn component_arguments(
    node: &KdlNode,
    maximum: usize,
) -> Result<Vec<String>, DesktopProfileError> {
    if node.entries().is_empty()
        || node.entries().len() > maximum
        || node.children().is_some()
        || node.ty().is_some()
    {
        return Err(schema_error(
            "component requires an absolute path and bounded arguments",
        ));
    }
    let mut arguments = Vec::with_capacity(node.entries().len());
    for entry in node.entries() {
        let value = entry
            .value()
            .as_string()
            .filter(|value| {
                value.len() <= crate::SOPHIA_CONFIG_MAX_ARGUMENT_BYTES
                    && !value.chars().any(char::is_control)
            })
            .ok_or_else(|| {
                schema_error("component arguments must be bounded strings without controls")
            })?;
        if entry.name().is_some() || entry.ty().is_some() {
            return Err(schema_error(
                "component arguments must be positional and untyped",
            ));
        }
        arguments.push(value.to_owned());
    }
    if !std::path::Path::new(&arguments[0]).is_absolute() {
        return Err(schema_error("component path must be absolute"));
    }
    Ok(arguments)
}

pub fn prepare_desktop_session_candidate(
    candidate: &DesktopAuthorityCandidate,
) -> Result<DesktopSessionCandidate, DesktopProfileError> {
    if candidate.authority != DesktopAuthority::Session {
        return Err(schema_error("candidate crossed its authority boundary"));
    }
    let mut prepared = DesktopSessionCandidate {
        generation: candidate.generation,
        digest: candidate.digest,
        terminal: None,
        browser: None,
        application_catalog: None,
        startup: None,
        logout_enabled: None,
        control: DesktopControlAccess::Disabled,
        inspection: DesktopInspectionAccess::Disabled,
        components: crate::DesktopComponents::default(),
        applications: Vec::new(),
    };
    for value in &candidate.values {
        let node = single_node(&value.encoded)?;
        match node.name().value() {
            "application" => {
                let application =
                    crate::application_command::parse_application(&node, &value.provenance)?;
                if prepared.applications.len() >= crate::SOPHIA_CONFIG_MAX_APPLICATIONS
                    || prepared
                        .applications
                        .iter()
                        .any(|old| old.name == application.name)
                {
                    return Err(schema_error(
                        "duplicate or excessive application registry entries",
                    ));
                }
                prepared.applications.push(application);
            }
            "terminal" => prepared.terminal = Some(application_name(&node, "terminal")?),
            "browser" => prepared.browser = Some(application_name(&node, "browser")?),
            "application-catalog" => {
                prepared.application_catalog = Some(application_name(&node, "application-catalog")?)
            }
            "startup" => {
                prepared.startup = Some(application_names(
                    &node,
                    "startup",
                    crate::SOPHIA_CONFIG_MAX_APPLICATIONS,
                )?)
            }
            "logout" => prepared.logout_enabled = Some(logout_enabled(&node)?),
            "window-manager" => {
                let mut arguments = component_arguments(&node, crate::SOPHIA_CONFIG_MAX_ARGUMENTS)?;
                prepared.components.window_manager = Some(crate::ExternalWmConfig {
                    executable: arguments.remove(0).into(),
                    arguments,
                    interface: crate::ExternalWmInterface::SophiaWmV1,
                });
            }
            "shell-client" => {
                prepared.components.shell_client =
                    Some(component_arguments(&node, 1)?.remove(0).into())
            }
            "shell-component" => {
                let component = crate::shell_components::parse(&node)?;
                let components = &mut prepared.components.shell_components;
                if components.len() >= crate::MAX_SHELL_COMPONENTS
                    || components
                        .iter()
                        .any(|old| old.id == component.id || old.role == component.role)
                {
                    return Err(schema_error(
                        "duplicate shell component identity, exclusive role or excessive component count",
                    ));
                }
                components.push(component);
            }
            "shell-config" => {
                prepared.components.shell_config =
                    Some(component_arguments(&node, 1)?.remove(0).into())
            }
            "control" => {
                if node.entries().len() != 1 || node.children().is_some() || node.ty().is_some() {
                    return Err(schema_error("control requires one access mode"));
                }
                prepared.control = match node.get(0).and_then(|value| value.as_string()) {
                    Some("disabled") => DesktopControlAccess::Disabled,
                    Some("host-admin") => DesktopControlAccess::HostAdmin,
                    _ => return Err(schema_error("control must be disabled or host-admin")),
                };
            }
            "inspection" => {
                if node.entries().len() != 1
                    || node.children().is_some()
                    || node.ty().is_some()
                    || node.entries()[0].name().is_some()
                    || node.entries()[0].ty().is_some()
                {
                    return Err(schema_error("inspection requires one untyped access mode"));
                }
                prepared.inspection = match node.get(0).and_then(|value| value.as_string()) {
                    Some("disabled") => DesktopInspectionAccess::Disabled,
                    Some("host-admin") => DesktopInspectionAccess::HostAdmin,
                    _ => return Err(schema_error("inspection must be disabled or host-admin")),
                };
            }
            _ => return Err(schema_error("candidate contains a non-session setting")),
        }
    }
    if !prepared.components.shell_components.is_empty()
        && (prepared.components.shell_client.is_some()
            || prepared.components.shell_config.is_some())
    {
        return Err(schema_error(
            "shell-component cannot be combined with legacy shell-client or shell-config",
        ));
    }
    crate::validate_shell_component_reservations(&prepared.components.shell_components)?;
    Ok(prepared)
}
