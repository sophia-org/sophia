#[path = "config/chrome.rs"]
mod chrome;
#[path = "config/firefox_stage.rs"]
mod firefox_stage;
#[path = "config/input_profile.rs"]
mod input_profile;
#[path = "config/output.rs"]
mod output;
#[path = "config/output_proof.rs"]
mod output_proof;
#[path = "config/reload.rs"]
mod reload;
#[path = "config/session.rs"]
mod session;
#[path = "config/session_profile.rs"]
mod session_profile;
#[path = "config/wm_proof.rs"]
mod wm_proof;
use firefox_stage::FirefoxM8StageProof;
use input_profile::PreparedInputProfile;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum WmTransportSelection {
    #[default]
    CurrentIpc,
    NineP2000L,
}
impl WmTransportSelection {
    const fn wire_name(self) -> &'static str {
        match self { Self::CurrentIpc => "sophia_wm_v1", Self::NineP2000L => "sophia_wm_fs_v1" }
    }
    const fn socket_env(self) -> &'static str {
        match self {
            Self::CurrentIpc => sophia_runtime::SOPHIA_WM_SOCKET_ENV,
            Self::NineP2000L => "SOPHIA_WM_9P_SOCKET",
        }
    }
}
use crate::desktop_output_publication::{
    output_topology_from_authority_at_generation, prepare_output_topology_publication,
};
use output::{
    LiveOutputAuthorityBootstrap, PreparedOutputProfile, output_topology_from_engine_outputs,
    output_topology_from_engine_outputs_at_generation,
    resolved_output_bounds, wm_output_bounds,
    wm_root_bounds,
};
use output_proof::{
    OutputProofRollbackAfterApply, parse_output_proof_rollback_after_apply,
    validate_prepared_output_proof_candidate,
};
use session::{
    SessionApplicationConfig, SessionApplicationOverrides, SessionApplicationSpec,
    session_action_evidence_name,
};
use session_profile::PreparedSessionProfile;

const TERMINAL_APPLICATION_ID: SessionApplicationId = SessionApplicationId::from_raw(1);
const LAUNCHER_APPLICATION_ID: SessionApplicationId = SessionApplicationId::from_raw(2);
const BROWSER_APPLICATION_ID: SessionApplicationId = SessionApplicationId::from_raw(3);

#[derive(Clone, Debug)]
struct PersistentXtermSessionConfig {
    display: String,

    socket_path: std::path::PathBuf,
    terminal: String,
    terminal_exec: Option<String>,
    terminal_exec_args: Vec<String>,
    session_launcher: Option<String>,
    session_browser: Option<String>,
    client: Option<String>,
    client_args: Vec<String>,
    expect_client_stdout: Option<String>,
    require_client_normal_exit: bool,
    normal_session: bool,
    exit_when_startup_exits: bool,
    startup_ready_timeout: Option<Duration>,
    applications: SessionApplicationConfig,
    session_application_overrides: SessionApplicationOverrides,
    session_profile: PreparedSessionProfile,
    active_launch_profile: Option<sophia_config::DesktopSessionCandidate>,
    control_access: sophia_config::DesktopControlAccess,
    control_socket: Option<std::path::PathBuf>,
    inspection_access: sophia_config::DesktopInspectionAccess,
    inspection_socket: Option<std::path::PathBuf>,
    application_catalog: Option<sophia_config::ApplicationCatalogConfig>,
    secondary_terminal: bool,
    max_runtime: Option<Duration>,
    max_ticks: Option<usize>,
    inject_text: Option<String>,
    expect_physical_text: Option<String>,
    physical_sequence_timeout_msec: u64,
    expect_physical_pointer: bool,
    /// Admit XTEST for clients in the session's namespace. A dev flag, off
    /// by default, and never beside a physical proof: a synthetic source
    /// could satisfy one, and rehearsal is not acceptance.
    admit_xtest: bool,
    exit_after_input_proof: bool,
    input_devices: Vec<std::path::PathBuf>,
    input_seat: Option<String>,
    native_scanout: bool,
    software_client_rendering: bool,
    wm_process: Option<String>,
    wm_process_args: Vec<String>,
    wm_process_executable_grants: Vec<std::path::PathBuf>,
    shell_process: Option<String>,
    /// Resolved provider capability shared by startup and launch-only reloads.
    shell_shortcuts_enabled: bool,
    shell_config: Option<std::path::PathBuf>,
    shell_panel_thickness: Option<u16>,
    shell_content_enabled: bool,
    shell_content_input_enabled: bool,
    shell_gpu_mode: sophia_config::ShellGpuMode,
    shell_proof_restart_after_visible: Option<u32>,
    wm_interface: sophia_config::ExternalWmInterface,
    wm_transport: WmTransportSelection,
    wm_public_fault_after: Option<PublicPolicyFaultPoint>,
    wm_public_restart_after_action: Option<WmActionId>,
    output_proof_rollback_after_apply: bool,
    wm_socket_path: std::path::PathBuf,
    input_quiet_msec: u64,
    namespace_profile: NamespaceProfile,
    namespace_capabilities: NamespaceCapabilities,
    xkb_config: sophia_x_authority::XkbRmlvoConfig,
    /// Directories the X frontend searches for core fonts.
    ///
    /// Defaults to the standard X11 font directories that exist on this host,
    /// the same set XLibre compiles in. `--font-path=` with nothing after it
    /// selects none, leaving only the built-in face, which is what a
    /// deterministic proof wants.
    font_path: Vec<std::path::PathBuf>,
    key_repeat_config: sophia_config::RepeatConfig,
    initial_caps_lock: bool,
    initial_num_lock: bool,
    shortcut_profile_candidate: sophia_config::DesktopShortcutCandidate,
    /// Whether the compiled default profile's shell was turned off because
    /// this session has nothing to run it with.
    shell_dropped: bool,
    /// Shortcuts the compiled default profile named that this session cannot
    /// perform, dropped rather than refused. Empty for an explicit profile,
    /// which refuses instead.
    dropped_shortcuts: Vec<sophia_config::DesktopSessionShortcut>,
    input_profile: PreparedInputProfile,
    output_profile: PreparedOutputProfile,
    desktop_profile: sophia_config::DesktopProfileGeneration,
    /// Where the desktop profile was read from, kept so it can be read again.
    /// A reload has to return to the same file the session started from; a
    /// discovery run at reload time could answer differently and silently
    /// swap which profile the desktop obeys.
    desktop_profile_source: Option<std::path::PathBuf>,
    desktop_profile_activation: sophia_config::DesktopProfileActivationModel,
    core_config_source: sophia_config::ConfigSource,
    core_config_state: sophia_config::CoreConfigState,
    surface_chrome_style: sophia_engine::SurfaceChromeStyle,
    cursor_resolution: sophia_renderer_live::CursorResolution,
    /// The same cursor rastered larger, for shake-to-find.
    ///
    /// Resolved once at startup rather than when the gesture fires: reading an
    /// XCursor file is filesystem work, and the pointer path is the last place
    /// that should be doing it. `None` means the gesture is off, or that the
    /// enlarged size is one this cursor or this hardware cannot show, in which
    /// case there is nothing to switch to and the session says so.
    cursor_shake_resolution: Option<sophia_renderer_live::CursorResolution>,
    verbose_diagnostics: bool,
    inject_output_size: Option<Size>,
    inject_surface_resize: Option<Size>,
    inject_surface_resize_sequence: Vec<Size>,
    m4_first_acquire_delay: Option<Duration>,
    m4_reject_first_present: bool,
    /// Whether this session drives an overlay over a directly scanned frame to
    /// prove the return to composition. Off in every product session.
    pub(crate) atomic_cursor: bool,
    pub(crate) direct_cursor_proof: bool,
    pub(crate) direct_overlay_proof: bool,
    /// How long the proof overlay stays up, in owner-loop ticks. Zero means
    /// the control's own default.
    pub(crate) direct_overlay_hold_ticks: u32,
    m4_diagnose_first_mixed_export: bool,
    firefox_m8_proof: bool,
    firefox_m10_proof: bool,
    firefox_m10_rendering_proof: bool,
    firefox_m10_dialog_proof: bool,
    firefox_m10_primary_proof: bool,
    firefox_m10_selection_proof: bool,
    firefox_m10_lifecycle_proof: bool,
}

/// Resolves the cursor the session draws, and the enlarged one a shake shows.
///
/// One function because startup and a core-config reload have to agree about
/// precedence. If they disagreed, editing the core config would quietly take a
/// cursor the profile had claimed, and only after a reload -- the kind of
/// difference nobody finds until they are looking at the wrong pointer.
///
/// The shape stays core-only: it names a semantic cursor the Engine draws,
/// which is not a thing a desktop profile has an opinion about.
fn resolve_desktop_cursor(
    desktop: Option<&sophia_config::DesktopCursorCandidate>,
    core: &sophia_config::CursorConfig,
) -> Result<
    (
        sophia_renderer_live::CursorResolution,
        Option<sophia_renderer_live::CursorResolution>,
    ),
    Box<dyn std::error::Error>,
> {
    let shape = sophia_engine::CursorShape::parse(&core.shape)
        .ok_or("validated core config has an unknown cursor shape")?;
    let theme = desktop
        .and_then(|cursor| cursor.theme.as_deref())
        .unwrap_or(&core.theme);
    let size = desktop.and_then(|cursor| cursor.size).unwrap_or(core.size);
    let base = sophia_renderer_live::resolve_cursor_theme(theme, size, shape, 0);
    let shake = desktop
        .is_some_and(|cursor| cursor.shake_to_find == Some(true))
        .then(|| {
            let enlarged = sophia_engine::CursorShakeDetector::enlarged_size(size);
            if enlarged <= size {
                // A cursor already at the size the Engine will raster cannot
                // grow, so there is nothing the gesture could do.
                crate::session_eprintln!(
                    "sophia_live_cursor schema=1 status=shake_to_find_inert reason=size_at_maximum size={size}"
                );
                return None;
            }
            let resolution = sophia_renderer_live::resolve_cursor_theme(theme, enlarged, shape, 0);
            crate::session_println!(
                "sophia_live_cursor schema=1 status=shake_to_find_ready base_size={size} enlarged_size={enlarged} effective_size={}",
                resolution.effective_nominal_size,
            );
            Some(resolution)
        })
        .flatten();
    Ok((base, shake))
}

impl PersistentXtermSessionConfig {
    /// Re-resolves the cursor after the core config changed.
    ///
    /// The desktop profile still wins per key, so a core edit to a theme the
    /// profile also names changes nothing -- which is the same answer startup
    /// would give, and the reason both go through one function.
    ///
    /// Returns the asset the backends should now draw, or `None` when the
    /// effective cursor did not actually change.
    pub(crate) fn reload_cursor(
        &mut self,
        core: &sophia_config::CursorConfig,
    ) -> Result<Option<sophia_engine::CursorAsset>, Box<dyn std::error::Error>> {
        let (base, shake) = resolve_desktop_cursor(self.input_profile.current().cursor.as_ref(), core)?;
        if base.asset.digest() == self.cursor_resolution.asset.digest() {
            self.cursor_shake_resolution = shake;
            return Ok(None);
        }
        let asset = base.asset.clone();
        self.cursor_resolution = base;
        self.cursor_shake_resolution = shake;
        Ok(Some(asset))
    }

    pub(super) fn keyboard_mapper(&self) -> XCoreKeyboardMapper {
        XCoreKeyboardMapper::with_locks(self.initial_caps_lock, self.initial_num_lock)
    }

    pub(super) fn native_pointer_policy(&self) -> sophia_backend_live::NativeLibinputPointerPolicy {
        let Some(candidate) = self.input_profile.current().pointer else {
            return sophia_backend_live::NativeLibinputPointerPolicy::default();
        };
        sophia_backend_live::NativeLibinputPointerPolicy {
            natural_scroll: candidate.natural_scroll,
            accel_profile: candidate.accel_profile.map(|profile| match profile {
                sophia_config::DesktopPointerAccelProfile::Flat => {
                    sophia_backend_live::NativeLibinputAccelProfile::Flat
                }
                sophia_config::DesktopPointerAccelProfile::Adaptive => {
                    sophia_backend_live::NativeLibinputAccelProfile::Adaptive
                }
            }),
            accel_speed: candidate.accel_speed,
            left_handed: candidate.left_handed,
            middle_emulation: candidate.middle_emulation,
            scroll_factor: candidate.scroll_factor.unwrap_or(1.0),
        }
    }

    fn applications_from_core(
        snapshot: &sophia_config::CoreConfigSnapshot,
    ) -> Result<SessionApplicationConfig, Box<dyn std::error::Error>> {
        let mut applications = SessionApplicationConfig::default();
        let mut names_by_id = BTreeMap::new();
        for app in &snapshot.session.applications {
            names_by_id.insert(app.id, app.name.clone());
            applications.applications.insert(
                app.name.clone(),
                SessionApplicationSpec {
                    id: app.name.clone(),
                    executable: app.executable.clone(),
                    arguments: app.arguments.clone(),
                    placement_classification: app.placement_classification,
                },
            );
            match app.id {
                1 => applications.terminal = Some(app.name.clone()),
                2 => applications.launcher = Some(app.name.clone()),
                3 => applications.browser = Some(app.name.clone()),
                _ => {}
            }
        }
        for id in &snapshot.session.startup {
            applications.startup.push(
                names_by_id
                    .get(id)
                    .ok_or_else(|| format!("core config startup references unknown app {id}"))?
                    .clone(),
            );
        }
        Ok(applications)
    }

    fn startup_proof_requested(&self) -> bool {
        !self.normal_session
            || self.startup_ready_timeout.is_some()
            || self.input_proof_requested()
            || self.application_proof_requested()
            || self.expect_physical_pointer
            || self.surface_resize_requested()
            || self.inject_output_size.is_some()
    }

    fn input_proof_requested(&self) -> bool {
        self.inject_text.is_some() || self.expect_physical_text.is_some()
    }

    fn surface_resize_requested(&self) -> bool {
        self.inject_surface_resize.is_some() || !self.inject_surface_resize_sequence.is_empty()
    }

    fn surface_resize_targets(&self) -> Vec<Size> {
        self.inject_surface_resize
            .iter()
            .copied()
            .chain(self.inject_surface_resize_sequence.iter().copied())
            .collect()
    }

    fn application_proof_requested(&self) -> bool {
        self.client.is_some()
    }

    fn application_for_action(&self, action: WmSessionAction) -> Option<&SessionApplicationSpec> {
        let id = match action {
            WmSessionAction::LaunchApplication { application }
                if application == TERMINAL_APPLICATION_ID =>
            {
                self.applications.terminal.as_ref()
            }
            WmSessionAction::LaunchApplication { application }
                if application == LAUNCHER_APPLICATION_ID =>
            {
                self.applications.launcher.as_ref()
            }
            WmSessionAction::LaunchApplication { application }
                if application == BROWSER_APPLICATION_ID =>
            {
                self.applications.browser.as_ref()
            }
            WmSessionAction::LaunchApplication { .. } => None,
            WmSessionAction::CloseFocused
            | WmSessionAction::Logout
            | WmSessionAction::ReloadProfile
            | WmSessionAction::RestartWm => None,
        }?;
        self.applications.applications.get(id)
    }
    fn spawn_session_application(
        app: &SessionApplicationSpec,
        display: &str,
        xauthority: &std::path::Path,
        control_socket: Option<&std::path::Path>,
        inspection_socket: Option<&std::path::Path>,
        context: crate::diagnostics::application::LaunchContext,
    ) -> Result<Child, Box<dyn std::error::Error>> {
        let mut command = std::process::Command::new(&app.executable);
        crate::application_catalog::configure_host_application_environment(
            &mut command,
            control_socket,
            inspection_socket,
        );
        command
            .args(&app.arguments)
            .env("DISPLAY", display)
            .env("XAUTHORITY", xauthority)
            .env_remove("ENV")
            .env_remove("BASH_ENV")
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        Ok(crate::diagnostics::application::spawn(&mut command, context)?)
    }

    fn launch_surface_proof_requested(&self) -> bool {
        self.firefox_proof_requested() || self.exit_after_input_proof || self.expect_physical_text.is_some()
    }

    fn firefox_proof_requested(&self) -> bool {
        self.firefox_m8_proof
            || self.firefox_m10_proof
            || self.firefox_m10_rendering_proof
            || self.firefox_m10_dialog_proof
            || self.firefox_m10_primary_proof
            || self.firefox_m10_selection_proof
            || self.firefox_m10_lifecycle_proof
    }

    fn firefox_full_proof_requested(&self) -> bool {
        self.firefox_m8_proof || self.firefox_m10_proof
    }
}

#[derive(Default)]
struct FirefoxM10KittyProof {
    observed: [bool; Self::CHECKPOINTS.len()],
}

#[derive(Default)]
struct FirefoxM10SelectionKittyProof {
    observed: [bool; Self::CHECKPOINTS.len()],
}

#[derive(Default)]
struct FirefoxM10DialogProof {
    completed: usize,
}

#[derive(Default)]
struct FirefoxM10PrimaryProof {
    completed: usize,
}

impl FirefoxM10PrimaryProof {
    // Page initialization can be coalesced before the metadata observer sees
    // it. A trusted full-field selection is the first causal proof boundary.
    const CHECKPOINTS: [(usize, &'static str); 3] = [
        (251, "source_armed"),
        (253, "kitty_received"),
        (252, "confirmed"),
    ];

    fn observe(&mut self, property_name: &str, byte_len: usize) -> Option<&'static str> {
        if property_name != "_NET_WM_NAME" {
            return None;
        }
        let (expected, checkpoint) = Self::CHECKPOINTS.get(self.completed)?;
        if byte_len != *expected {
            return None;
        }
        self.completed += 1;
        Some(*checkpoint)
    }

    fn complete(&self) -> bool {
        self.completed == Self::CHECKPOINTS.len()
    }
}

impl FirefoxM10DialogProof {
    const CHECKPOINTS: [(usize, &'static str); 3] = [
        (245, "page_ready"),
        (246, "modal_ready"),
        (247, "confirmed"),
    ];

    fn observe(&mut self, property_name: &str, byte_len: usize) -> Option<&'static str> {
        if property_name != "_NET_WM_NAME" {
            return None;
        }
        let (expected, checkpoint) = Self::CHECKPOINTS.get(self.completed)?;
        if byte_len != *expected {
            return None;
        }
        self.completed += 1;
        Some(*checkpoint)
    }

    fn complete(&self) -> bool {
        self.completed == Self::CHECKPOINTS.len()
    }
}

impl FirefoxM10SelectionKittyProof {
    const CHECKPOINTS: [(usize, &'static str); 3] = [
        (241, "before"),
        (242, "clipboard_peer"),
        (243, "primary_peer"),
    ];

    fn observe(&mut self, property_name: &str, byte_len: usize) -> Option<&'static str> {
        if property_name != "_NET_WM_NAME" {
            return None;
        }
        let (index, (_, checkpoint)) = Self::CHECKPOINTS
            .iter()
            .enumerate()
            .find(|(_, (expected, _))| *expected == byte_len)?;
        if self.observed[index] {
            return None;
        }
        self.observed[index] = true;
        Some(*checkpoint)
    }

    fn complete(&self) -> bool {
        self.observed.iter().all(|observed| *observed)
    }

    fn completed(&self) -> usize {
        self.observed.iter().filter(|observed| **observed).count()
    }
}

impl FirefoxM10KittyProof {
    const CHECKPOINTS: [(usize, &'static str, &'static str); 6] = [
        (193, "a", "before"),
        (194, "b", "before"),
        (211, "a", "after_normal_close"),
        (212, "b", "after_normal_close"),
        (229, "a", "after_forced_close"),
        (230, "b", "after_forced_close"),
    ];

    fn observe(
        &mut self,
        property_name: &str,
        byte_len: usize,
    ) -> Option<(&'static str, &'static str)> {
        if property_name != "_NET_WM_NAME" {
            return None;
        }
        let (index, (_, terminal, checkpoint)) = Self::CHECKPOINTS
            .iter()
            .enumerate()
            .find(|(_, (expected, _, _))| *expected == byte_len)?;
        if self.observed[index] {
            return None;
        }
        self.observed[index] = true;
        Some((*terminal, *checkpoint))
    }

    fn complete(&self) -> bool {
        self.observed.iter().all(|observed| *observed)
    }

    fn lifecycle_complete(&self) -> bool {
        self.complete()
    }

    fn completed(&self) -> usize {
        self.observed.iter().filter(|observed| **observed).count()
    }
}

include!("config/arguments.rs");
