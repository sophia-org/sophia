include!("wm/policy_map.rs");
include!("wm/policy_session_directory.rs");
include!("wm/application_commands.rs");
include!("wm/shortcut_resolution.rs");
include!("wm/public_policy.rs");
include!("wm/profile_preparation.rs");
include!("wm/indicator_admission.rs");
include!("wm/output_policy_keys.rs");
include!("wm/session.rs");
include!("wm/control.rs");
include!("wm/chrome.rs");
include!("wm/overview.rs");
include!("wm/commit.rs");
include!("wm/visual_candidate.rs");
include!("wm/admission.rs");
include!("wm/layout.rs");
include!("wm/layout_support.rs");
include!("wm/work_area.rs");

impl LiveWmSession {
    fn reference_output(&self)->Option<sophia_protocol::OutputId> {
        self.public.as_ref().filter(|p|p.configured).map(|p|p.active_output)
    }

    fn reference_shortcuts(&self) -> Option<&sophia_config::DesktopShortcutCandidate> {
        let public=self.public.as_ref().filter(|p|p.configured)?;
        public.shortcut_profile_slot.active().or_else(||public.shortcut_profile_slot.candidate())
    }
}
