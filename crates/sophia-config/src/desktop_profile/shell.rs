use kdl::KdlDocument;

use super::{DesktopAuthority, DesktopProfileGeneration, ShellGpuMode};

/// Returns the exact prepared shell-owner enablement decision.
///
/// Desktop-profile validation requires one boolean `shell.enabled` value. The
/// compiled profile supplies it when no external profile does, so absence here
/// is a conservative disabled result rather than a second default.
pub fn desktop_profile_shell_enabled(profile: &DesktopProfileGeneration) -> bool {
    profile
        .candidates
        .get(&DesktopAuthority::Shell)
        .and_then(|candidate| {
            candidate
                .values
                .iter()
                .find(|value| value.key == "shell.enabled")
        })
        .and_then(|value| KdlDocument::parse_v2(&value.encoded).ok())
        .and_then(|document| {
            (document.nodes().len() == 1)
                .then(|| document.nodes()[0].get(0).and_then(|value| value.as_bool()))
                .flatten()
        })
        .unwrap_or(false)
}

/// Returns the explicit production content decision. Absence is denial.
pub fn desktop_profile_shell_content_enabled(profile: &DesktopProfileGeneration) -> bool {
    profile
        .candidates
        .get(&DesktopAuthority::Shell)
        .and_then(|candidate| {
            candidate
                .values
                .iter()
                .find(|value| value.key == "shell.content")
        })
        .and_then(|value| KdlDocument::parse_v2(&value.encoded).ok())
        .and_then(|document| {
            (document.nodes().len() == 1)
                .then(|| document.nodes()[0].get(0).and_then(|value| value.as_bool()))
                .flatten()
        })
        .unwrap_or(false)
}

/// Returns the explicit permission for target-bound discrete shell actions.
///
/// This is independent of ordinary pointer routing. Absence is denial, and
/// Session additionally requires the content owner itself to be enabled.
pub fn desktop_profile_shell_content_input_enabled(profile: &DesktopProfileGeneration) -> bool {
    profile
        .candidates
        .get(&DesktopAuthority::Shell)
        .and_then(|candidate| {
            candidate
                .values
                .iter()
                .find(|value| value.key == "shell.content-input")
        })
        .and_then(|value| KdlDocument::parse_v2(&value.encoded).ok())
        .and_then(|document| {
            (document.nodes().len() == 1)
                .then(|| document.nodes()[0].get(0).and_then(|value| value.as_bool()))
                .flatten()
        })
        .unwrap_or(false)
}

/// Returns the explicit shell GPU execution policy. Absence is denial.
pub fn desktop_profile_shell_gpu_mode(profile: &DesktopProfileGeneration) -> ShellGpuMode {
    profile
        .candidates
        .get(&DesktopAuthority::Shell)
        .and_then(|candidate| {
            candidate
                .values
                .iter()
                .find(|value| value.key == "shell.gpu")
        })
        .and_then(|value| KdlDocument::parse_v2(&value.encoded).ok())
        .and_then(|document| {
            (document.nodes().len() == 1)
                .then(|| {
                    document.nodes()[0]
                        .get(0)
                        .and_then(|value| value.as_string())
                        .and_then(|mode| match mode {
                            "denied" => Some(ShellGpuMode::Denied),
                            "direct" => Some(ShellGpuMode::Direct),
                            _ => None,
                        })
                })
                .flatten()
        })
        .unwrap_or_default()
}

/// Returns the prepared shell panel thickness in pixels, if the profile asks
/// for one.
///
/// `None` means this session reserves no work area for a panel, which is what
/// every profile written before the key existed says, and what a profile that
/// asks for zero says too: a zero-thickness strip is not a claim.
pub fn desktop_profile_shell_panel_thickness(profile: &DesktopProfileGeneration) -> Option<u16> {
    profile
        .candidates
        .get(&DesktopAuthority::Shell)
        .and_then(|candidate| {
            candidate
                .values
                .iter()
                .find(|value| value.key == "shell.panel")
        })
        .and_then(|value| KdlDocument::parse_v2(&value.encoded).ok())
        .and_then(|document| {
            (document.nodes().len() == 1)
                .then(|| {
                    document.nodes()[0]
                        .get(0)
                        .and_then(|value| value.as_integer())
                })
                .flatten()
        })
        .and_then(|thickness| u16::try_from(thickness).ok())
        .filter(|thickness| *thickness > 0)
}
