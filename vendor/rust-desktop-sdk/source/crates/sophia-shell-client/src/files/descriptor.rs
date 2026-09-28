//! Descriptor admission and per-family capability checks. The advertised
//! role is checked independently of capability bits used by content bars.
use super::FileWire;
use crate::{ShellClientError, ShellClientOptions};
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

pub(super) fn validate_negotiated(
    options: &ShellClientOptions,
    value: &ShellFileNegotiated,
) -> Result<(), ShellClientError> {
    let caps = value.welcome.capabilities;
    let revision = value.welcome.selected_revision;
    let mut allowed =
        SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION;
    if revision >= 2 {
        allowed |= SOPHIA_SHELL_CAPABILITY_TAB_GROUPS;
    }
    if revision >= 3 {
        allowed |=
            SOPHIA_SHELL_CAPABILITY_SHORTCUT_CATALOG | SOPHIA_SHELL_CAPABILITY_REFERENCE_SHEET;
    }
    if revision >= 4 {
        allowed |= SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
            | SOPHIA_SHELL_CAPABILITY_APPLICATION_LAUNCHER;
    }
    if revision >= 5 {
        allowed |= SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
    }
    if revision >= 6 {
        allowed |=
            SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION;
    }
    if revision > 8
        || options.required_capabilities & SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER == 0
        || caps != options.required_capabilities | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
        || caps & !allowed != 0
    {
        return Err(ShellClientError::MissingCapability);
    }
    for (dependent, prerequisite) in [
        (
            SOPHIA_SHELL_CAPABILITY_REFERENCE_SHEET,
            SOPHIA_SHELL_CAPABILITY_SHORTCUT_CATALOG,
        ),
        (
            SOPHIA_SHELL_CAPABILITY_APPLICATION_LAUNCHER,
            SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG,
        ),
        (
            SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT,
            SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
        ),
        (
            SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION,
            SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS,
        ),
    ] {
        if caps & dependent != 0 && caps & prerequisite == 0 {
            return Err(ShellClientError::MissingCapability);
        }
    }
    if value.limits_published != (caps & SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE != 0) {
        return Err(ShellClientError::Protocol(
            "descriptor content grant and Limits disagree",
        ));
    }
    Ok(())
}

impl FileWire {
    pub(super) fn require_descriptor_kind(
        &self,
        kind: ShellFileKind,
    ) -> Result<(), ShellClientError> {
        let needed = match kind {
            ShellFileKind::Descriptors
            | ShellFileKind::DescriptorOutcome
            | ShellFileKind::DescriptorActivation
            | ShellFileKind::DescriptorCandidate
            | ShellFileKind::DescriptorActivationAck => SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER,
            ShellFileKind::Tabs | ShellFileKind::TabsCandidate => {
                SOPHIA_SHELL_CAPABILITY_TAB_GROUPS
            }
            ShellFileKind::Shortcuts => SOPHIA_SHELL_CAPABILITY_SHORTCUT_CATALOG,
            ShellFileKind::ReferenceRequest
            | ShellFileKind::ReferenceOutcome
            | ShellFileKind::ReferenceCandidate => SOPHIA_SHELL_CAPABILITY_REFERENCE_SHEET,
            ShellFileKind::LauncherRequest
            | ShellFileKind::LauncherOutcome
            | ShellFileKind::LauncherActivation
            | ShellFileKind::LaunchOutcome
            | ShellFileKind::LauncherCandidate
            | ShellFileKind::LauncherActivationAck => SOPHIA_SHELL_CAPABILITY_APPLICATION_LAUNCHER,
            _ => return Err(ShellClientError::WrongDirection),
        };
        if !self.descriptor || self.capabilities & needed == 0 {
            return Err(ShellClientError::MissingCapability);
        }
        Ok(())
    }
}
