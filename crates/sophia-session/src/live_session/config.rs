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
    application_catalog: Option<sophia_config::ApplicationCatalogConfig>,
    secondary_terminal: bool,
    max_runtime: Option<Duration>,
    max_ticks: Option<usize>,
    inject_text: Option<String>,
    expect_physical_text: Option<String>,
    physical_sequence_timeout_msec: u64,
    expect_physical_pointer: bool,
    exit_after_input_proof: bool,
    input_devices: Vec<std::path::PathBuf>,
    input_seat: Option<String>,
    native_scanout: bool,
    software_client_rendering: bool,
    wm_process: Option<String>,
    wm_process_args: Vec<String>,
    wm_process_executable_grants: Vec<std::path::PathBuf>,
    shell_process: Option<String>,
    shell_config: Option<std::path::PathBuf>,
    shell_panel_thickness: Option<u16>,
    shell_content_enabled: bool,
    shell_content_input_enabled: bool,
    shell_gpu_mode: sophia_config::ShellGpuMode,
    shell_proof_restart_after_visible: Option<u32>,
    wm_interface: sophia_config::ExternalWmInterface,
    wm_public_fault_after: Option<PublicPolicyFaultPoint>,
    wm_public_restart_after_action: Option<WmActionId>,
    output_proof_rollback_after_apply: bool,
    wm_socket_path: std::path::PathBuf,
    input_quiet_msec: u64,
    namespace_profile: NamespaceProfile,
    namespace_capabilities: NamespaceCapabilities,
    xkb_config: sophia_x_authority::XkbRmlvoConfig,
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

    fn from_args(args: &[String]) -> Result<Self, Box<dyn std::error::Error>> {
        let no_config = args.iter().any(|argument| argument == "--no-config");
        let explicit_config = arg_value(args, "--config")
            .map(std::path::PathBuf::from);
        if no_config && explicit_config.is_some() {
            return Err("--no-config and --config are mutually exclusive".into());
        }
        if explicit_config
            .as_ref()
            .is_some_and(|path| !path.is_absolute() || path.as_os_str().is_empty())
        {
            return Err("--config requires an absolute path".into());
        }
        let core_config_source = if no_config {
            sophia_config::ConfigSource {
                class: sophia_config::ConfigSourceClass::CompiledDefault,
                path: None,
            }
        } else {
            sophia_config::discover_default_config_source(
                sophia_config::ConfigDomain::Core,
                explicit_config.as_deref(),
            )
        };
        let core_config_state = sophia_config::CoreConfigState::load(&core_config_source)?;
        let core_snapshot = core_config_state.active();
        let explicit_desktop_profile = arg_value(args, "--desktop-profile").map(Into::into);
        if no_config && explicit_desktop_profile.is_some() {
            return Err("--no-config and --desktop-profile are mutually exclusive".into());
        }
        if explicit_desktop_profile
            .as_ref()
            .is_some_and(|path: &std::path::PathBuf| !path.is_absolute())
        {
            return Err("--desktop-profile requires an absolute path".into());
        }
        let user_config_root = sophia_config::default_user_config_root();
        let desktop_profile_source = if no_config {
            None
        } else {
            sophia_config::discover_desktop_profile_source(
                explicit_desktop_profile.as_deref(),
                user_config_root.as_deref(),
            )
        };
        let sophia_config::PreparedDesktopProfile {
            profile: desktop_profile,
            candidates: prepared_desktop,
        } = sophia_config::load_prepared_desktop_profile(
            desktop_profile_source.as_deref(),
            sophia_config::ConfigGeneration::INITIAL,
        )?;
        let shell_enabled = sophia_config::desktop_profile_shell_enabled(&desktop_profile);
        let sophia_config::PreparedDesktopProfileCandidates {
            shortcut: shortcut_profile_candidate,
            session: session_profile_candidate,
            input: input_profile_candidate,
            output: output_profile_candidate,
        } = prepared_desktop;
        // The typed profile must not silently fall back to the legacy shell
        // while the shared component admission/transport owner is unfinished.
        if !session_profile_candidate.components.shell_components.is_empty() {
            return Err("independent shell components require revision-7 Session admission, which is not implemented".into());
        }
        let session_profile = PreparedSessionProfile::new(session_profile_candidate)?;
        let input_profile = PreparedInputProfile::new(input_profile_candidate)?;
        let output_profile = PreparedOutputProfile::new(output_profile_candidate)?;
        let desktop_input = input_profile.current();
        // The profile overrides the core config rather than replacing it, so a
        // desktop that states only a theme keeps the core size and a desktop
        // that states no cursor at all leaves the core config in charge. That
        // is what lets one file describe a whole desktop without making the
        // core config unusable for a session that has no profile.
        let (cursor_resolution, cursor_shake_resolution) =
            resolve_desktop_cursor(desktop_input.cursor.as_ref(), &core_snapshot.cursor)?;
        let display = arg_value(args, "--display").unwrap_or_else(|| ":77".to_owned());
        let display_number = parse_display_number(&display)?;
        let normal_session = args.iter().any(|arg| arg == "--session-mode=normal")
            || !core_snapshot.session.applications.is_empty()
            || !session_profile.candidate().applications.is_empty();
        let exit_when_startup_exits = args.iter().any(|arg| arg == "--exit-when-startup-exits");
        let startup_ready_timeout = arg_value(args, "--startup-ready-timeout-ms")
            .as_deref()
            .map(parse_u64)
            .transpose()?
            .map(Duration::from_millis);
        if startup_ready_timeout.is_some_and(|timeout| {
            timeout < Duration::from_millis(100) || timeout > Duration::from_secs(60)
        }) {
            return Err("--startup-ready-timeout-ms accepts 100-60000 milliseconds".into());
        }
        let session_profile_candidate = session_profile.candidate();
        let session_application_overrides = SessionApplicationOverrides::parse(args)?;
        let applications = session_application_overrides.prepare(
            Self::applications_from_core(core_snapshot)?,
            session_profile_candidate,
        )?;
        let application_catalog = session_profile_candidate.application_catalog.as_ref().map(|name| {
            core_snapshot.session.application_catalogs.iter().find(|c| &c.name == name).cloned()
                .ok_or("desktop selects an unknown application catalog")
        }).transpose()?;
        // An explicit empty login list has no application whose first frame
        // could satisfy the launcher's application watchdog.
        let startup_ready_timeout = if normal_session && applications.startup.is_empty()
            && session_profile_candidate.startup.as_ref().is_some_and(Vec::is_empty)
        { None } else { startup_ready_timeout };
        if normal_session {
            let terminal_proof = args.iter().any(|arg| {
                arg.starts_with("--inject-text=") || arg.starts_with("--expect-physical-text=")
            });
            let startup_terminal = applications.startup.len() == 1
                && applications.terminal.as_ref() == applications.startup.first();
            let proof_only = args.iter().any(|arg| {
                arg == "--secondary-terminal"
                    || arg == "--proof"
                    || arg.starts_with("--client=")
                    || arg.starts_with("--terminal=")
                    || arg.starts_with("--terminal-exec=")
                    || ((arg.starts_with("--inject-text=")
                        || arg.starts_with("--expect-physical-text="))
                        && !startup_terminal)
            });
            if proof_only {
                return Err(
                    "--session-mode=normal cannot be combined with proof or terminal-specific options"
                        .into(),
                );
            }
            if exit_when_startup_exits && applications.startup.len() != 1 {
                return Err(
                    "--exit-when-startup-exits requires exactly one normal-session startup app"
                        .into(),
                );
            }
            if startup_ready_timeout.is_some() && applications.startup.is_empty() {
                return Err(
                    "--startup-ready-timeout-ms requires at least one startup application".into(),
                );
            }
            if terminal_proof && !startup_terminal {
                return Err(
                    "normal-session input proof requires one startup app mapped to the terminal action"
                        .into(),
                );
            }
        } else if !applications.applications.is_empty()
            || !applications.startup.is_empty()
            || applications.terminal.is_some()
            || applications.launcher.is_some()
            || applications.browser.is_some()
        {
            return Err("session application options require --session-mode=normal".into());
        } else if exit_when_startup_exits {
            return Err("--exit-when-startup-exits requires --session-mode=normal".into());
        } else if startup_ready_timeout.is_some() {
            return Err("--startup-ready-timeout-ms requires --session-mode=normal".into());
        }
        let max_runtime = arg_value(args, "--max-runtime-ms")
            .as_deref()
            .map(parse_u64)
            .transpose()?
            .map(Duration::from_millis);
        let max_ticks = arg_value(args, "--max-ticks")
            .as_deref()
            .map(parse_usize)
            .transpose()?;
        if max_ticks.is_some_and(|ticks| ticks == 0 || ticks > 1_000_000) {
            return Err("--max-ticks accepts a value from 1 through 1000000".into());
        }
        let inject_text = arg_value(args, "--inject-text");
        let expect_physical_text = arg_value(args, "--expect-physical-text");
        let physical_sequence_timeout = arg_value(args, "--physical-sequence-timeout-ms")
            .as_deref()
            .map(parse_u64)
            .transpose()?;
        if physical_sequence_timeout
            .is_some_and(|timeout| !(1_000..=600_000).contains(&timeout))
        {
            return Err(
                "--physical-sequence-timeout-ms accepts a value from 1000 through 600000"
                    .into(),
            );
        }
        if physical_sequence_timeout.is_some() && expect_physical_text.is_none() {
            return Err(
                "--physical-sequence-timeout-ms requires --expect-physical-text".into(),
            );
        }
        let terminal_exec = arg_value(args, "--terminal-exec");
        let terminal_exec_args = args
            .iter()
            .filter_map(|arg| arg.strip_prefix("--terminal-exec-arg="))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let session_launcher = arg_value(args, "--session-launcher");
        let session_browser = arg_value(args, "--session-browser");
        if session_launcher
            .iter()
            .chain(session_browser.iter())
            .any(|path| path.is_empty() || path.len() > 4_096)
        {
            return Err("approved session executable paths accept 1-4096 bytes".into());
        }
        let client = arg_value(args, "--client");
        let software_client_rendering = args.iter().any(|arg| arg == "--software-client-rendering");
        let client_args = args
            .iter()
            .filter_map(|arg| arg.strip_prefix("--client-arg="))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let expect_client_stdout = arg_value(args, "--expect-client-stdout");
        let require_client_normal_exit =
            args.iter().any(|arg| arg == "--require-client-normal-exit");
        let proof_mode = args.iter().any(|arg| arg == "--proof");
        if software_client_rendering && client.is_none() {
            return Err("--software-client-rendering requires --client".into());
        }
        if client.is_none()
            && (!client_args.is_empty()
                || expect_client_stdout.is_some()
                || require_client_normal_exit)
        {
            return Err("client proof options require --client".into());
        }
        if client_args.len() > 64 || client_args.iter().any(|argument| argument.len() > 4_096) {
            return Err("--client accepts at most 64 bounded arguments".into());
        }
        if expect_client_stdout
            .as_ref()
            .is_some_and(|text| text.len() > 4_096)
        {
            return Err("--expect-client-stdout accepts at most 4096 bytes".into());
        }
        if terminal_exec.is_none() && !terminal_exec_args.is_empty() {
            return Err("--terminal-exec-arg requires --terminal-exec".into());
        }
        if terminal_exec_args.len() > 32
            || terminal_exec_args
                .iter()
                .any(|argument| argument.len() > 4_096)
        {
            return Err("--terminal-exec accepts at most 32 bounded arguments".into());
        }
        let expect_physical_pointer = args.iter().any(|arg| arg == "--expect-physical-pointer");
        let secondary_terminal = args.iter().any(|arg| arg == "--secondary-terminal");
        let exit_after_input_proof = args.iter().any(|arg| arg == "--exit-after-input-proof");
        let native_scanout = args.iter().any(|arg| arg == "--native-scanout");
        let namespace_profile_name = arg_value(args, "--namespace-profile")
            .unwrap_or_else(|| core_snapshot.namespace_profile.clone());
        let namespace_profile = match namespace_profile_name.as_str() {
            "classic" | "classic-shared" => NamespaceProfile::ClassicShared,
            "confined" => NamespaceProfile::Confined,
            profile => {
                return Err(format!(
                    "unsupported namespace profile {profile:?}; expected classic or confined"
                )
                .into());
            }
        };
        let mut effective_xkb = if desktop_input.inherit_sophia {
            core_snapshot.input.xkb.clone()
        } else {
            sophia_config::XkbConfig::default()
        };
        let mut key_repeat_config = if desktop_input.inherit_sophia {
            core_snapshot.input.repeat
        } else {
            sophia_config::RepeatConfig::default()
        };
        let mut initial_caps_lock = false;
        let mut initial_num_lock = false;
        if let Some(keyboard) = desktop_input.keyboard.as_ref() {
            if let Some(xkb) = keyboard.xkb.as_ref() {
                if let Some(value) = xkb.rules.as_ref() {
                    effective_xkb.rules.clone_from(value);
                }
                if let Some(value) = xkb.model.as_ref() {
                    effective_xkb.model.clone_from(value);
                }
                if let Some(value) = xkb.layout.as_ref() {
                    effective_xkb.layout.clone_from(value);
                }
                if let Some(value) = xkb.variant.as_ref() {
                    effective_xkb.variant.clone_from(value);
                }
                if let Some(value) = xkb.options.as_ref() {
                    effective_xkb.options.clone_from(value);
                }
            }
            if let Some(rate) = keyboard.repeat_rate {
                key_repeat_config.interval_msec = u64::from(1_000_u32.div_ceil(rate));
            }
            if let Some(delay) = keyboard.repeat_delay_msec {
                key_repeat_config.delay_msec = delay;
            }
            initial_caps_lock = keyboard.caps_lock.unwrap_or(false);
            initial_num_lock = keyboard.num_lock.unwrap_or(false);
        }
        let xkb_config = sophia_x_authority::XkbRmlvoConfig {
            rules: arg_value(args, "--xkb-rules").unwrap_or(effective_xkb.rules),
            model: arg_value(args, "--xkb-model").unwrap_or(effective_xkb.model),
            layout: arg_value(args, "--xkb-layout").unwrap_or(effective_xkb.layout),
            variant: arg_value(args, "--xkb-variant").unwrap_or(effective_xkb.variant),
            options: arg_value(args, "--xkb-options").unwrap_or(effective_xkb.options),
        };
        xkb_config.validate()?;
        let inject_output_size = arg_value(args, "--inject-output-size")
            .as_deref()
            .map(parse_output_size)
            .transpose()?;
        let inject_surface_resize = arg_value(args, "--inject-surface-resize")
            .as_deref()
            .map(parse_output_size)
            .transpose()?;
        let inject_surface_resize_sequence =
            arg_value(args, "--inject-surface-resize-sequence")
                .as_deref()
                .map(parse_surface_resize_sequence)
                .transpose()?
                .unwrap_or_default();
        if inject_surface_resize.is_some() && !inject_surface_resize_sequence.is_empty() {
            return Err(
                "--inject-surface-resize and --inject-surface-resize-sequence are mutually exclusive"
                    .into(),
            );
        }
        let m4_first_acquire_delay = arg_value(args, "--m4-first-acquire-delay-ms")
            .as_deref()
            .map(parse_u64)
            .transpose()?
            .map(Duration::from_millis);
        if m4_first_acquire_delay.is_some_and(|delay| delay.is_zero() || delay.as_millis() > 2_000)
        {
            return Err("--m4-first-acquire-delay-ms accepts 1-2000 milliseconds".into());
        }
        let m4_reject_first_present = args.iter().any(|arg| arg == "--m4-reject-first-present");
        // The cursor rides an atomic plane by default. Archive 0006 showed the
        // owner combining primary and cursor state in one request on hardware,
        // and a continuous-motion shakedown held 58 fps with p95 inside one
        // refresh while doing in 56 commits the work the legacy ioctl took 298
        // to do -- 243 of which overlapped a page flip, which is the thing an
        // atomic commit cannot do and does not need to.
        //
        // Still a preference rather than a guarantee: the startup probe decides
        // per card, and one that refuses keeps the legacy ioctl, which is why
        // that path is a fallback rather than dead code.
        //
        // `--atomic-cursor` no longer selects anything, because the default
        // already does. It is retained as an assertion: a run that names it is
        // refused unless the session it asked for can honour it, so a harness
        // measuring the atomic path cannot quietly measure the legacy one.
        let asked_atomic_cursor = args.iter().any(|arg| arg == "--atomic-cursor");
        let legacy_cursor = args.iter().any(|arg| arg == "--legacy-cursor");
        let atomic_cursor = !legacy_cursor;
        // Moves the cursor over frames the plane is scanning directly, which is
        // how archive 0004 established the legacy baseline the atomic path had
        // to match rather than assuming the ioctl kept working there.
        let direct_cursor_proof = args.iter().any(|arg| arg == "--direct-cursor-proof");
        // Drives an overlay over a directly scanned frame, which no client in
        // this session can do: the probe runs no shell and no policy client,
        // and the shell is what opens the descriptor overlay in a product
        // session. The transition it exercises -- a composed successor retiring
        // a frame the plane is still scanning -- is modelled in
        // `PresentFlipOwnership.tla` and had never run on hardware before it.
        let direct_overlay_proof = args.iter().any(|arg| arg == "--direct-overlay-proof");
        // How long the overlay stays up. A run proving the transition wants
        // the shortest window that contains it; a run measuring what frames
        // cost wants a composed population big enough to have a
        // distribution, which at a cursor-blink repaint rate takes seconds.
        let direct_overlay_hold_ticks = arg_value(args, "--direct-overlay-hold-ticks")
            .as_deref()
            .map(parse_u64)
            .transpose()?
            .unwrap_or(0);
        if direct_overlay_hold_ticks > 60_000 {
            return Err("--direct-overlay-hold-ticks accepts at most 60000 ticks".into());
        }
        let direct_overlay_hold_ticks =
            u32::try_from(direct_overlay_hold_ticks).unwrap_or(u32::MAX);
        let m4_diagnose_first_mixed_export = args
            .iter()
            .any(|arg| arg == "--m4-diagnose-first-mixed-export");
        let firefox_m8_proof = args.iter().any(|arg| arg == "--firefox-m8-proof");
        let firefox_m10_proof = args.iter().any(|arg| arg == "--firefox-m10-proof");
        let firefox_m10_rendering_proof = args
            .iter()
            .any(|arg| arg == "--firefox-m10-rendering-proof");
        let firefox_m10_dialog_proof = args
            .iter()
            .any(|arg| arg == "--firefox-m10-dialog-proof");
        let firefox_m10_primary_proof = args
            .iter()
            .any(|arg| arg == "--firefox-m10-primary-proof");
        let firefox_m10_selection_proof = args
            .iter()
            .any(|arg| arg == "--firefox-m10-selection-proof");
        let firefox_m10_lifecycle_proof = args
            .iter()
            .any(|arg| arg == "--firefox-m10-lifecycle-proof");
        let firefox_proof_count = [
            firefox_m8_proof,
            firefox_m10_proof,
            firefox_m10_rendering_proof,
            firefox_m10_dialog_proof,
            firefox_m10_primary_proof,
            firefox_m10_selection_proof,
            firefox_m10_lifecycle_proof,
        ]
        .into_iter()
        .filter(|selected| *selected)
        .count();
        if firefox_proof_count > 1 {
            return Err("select only one Firefox proof mode".into());
        }
        if firefox_proof_count == 1
            && (!normal_session || applications.browser.is_none())
        {
            return Err(
                "Firefox proof mode requires normal session mode and a Firefox action mapping"
                    .into(),
            );
        }
        let components = &session_profile_candidate.components;
        let configured_wm = components.window_manager.as_ref()
            .or(core_snapshot.external_wm.as_ref());
        let explicit_wm_process = arg_value(args, "--wm-process");
        let default_wm_process = arg_value(args, "--wm-process-default");
        for process in [&explicit_wm_process, &default_wm_process].into_iter().flatten() {
            if !std::path::Path::new(process).is_absolute() {
                return Err("WM process selections require an absolute path".into());
            }
        }
        let wm_process = explicit_wm_process.clone().or_else(|| {
                configured_wm
                    .and_then(|wm| wm.executable.to_str())
                    .map(ToOwned::to_owned)
            }).or(default_wm_process);
        let mut wm_process_args = if explicit_wm_process.is_some() {
            Vec::new()
        } else {
            configured_wm
                .map(|wm| wm.arguments.clone())
                .unwrap_or_default()
        };
        if normal_session && wm_process.is_none() && applications.startup.is_empty()
            && applications.terminal.is_none() && applications.launcher.is_none()
            && applications.browser.is_none()
        {
            return Err("--session-mode=normal requires a WM, startup app, or session action mapping".into());
        }
        wm_process_args.extend(args
            .iter()
            .filter_map(|arg| arg.strip_prefix("--wm-process-arg="))
            .map(ToOwned::to_owned)
        );
        let wm_process_executable_grants = args
            .iter()
            .filter_map(|arg| arg.strip_prefix("--wm-process-executable-grant="))
            .map(std::path::PathBuf::from)
            .collect::<Vec<_>>();
        if wm_process.is_none() && !wm_process_args.is_empty() {
            return Err("--wm-process-arg requires --wm-process".into());
        }
        if wm_process.is_none() && !wm_process_executable_grants.is_empty() {
            return Err("--wm-process-executable-grant requires --wm-process".into());
        }
        if wm_process_executable_grants
            .iter()
            .any(|path| !path.is_absolute())
        {
            return Err("--wm-process-executable-grant requires an absolute path".into());
        }
        for executable in &wm_process_executable_grants {
            let metadata = std::fs::metadata(executable).map_err(|error| {
                format!(
                    "--wm-process-executable-grant cannot inspect {}: {error}",
                    executable.display()
                )
            })?;
            if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
                return Err(format!(
                    "--wm-process-executable-grant is not an executable file: {}",
                    executable.display()
                )
                .into());
            }
        }
        let wm_interface = match arg_value(args, "--wm-interface").as_deref() {
            Some("sophia_wm_v1") => sophia_config::ExternalWmInterface::SophiaWmV1,
            Some(other) => {
                return Err(format!("--wm-interface expects sophia_wm_v1, got {other:?}").into());
            }
            None => configured_wm.map_or(
                sophia_config::ExternalWmInterface::default(),
                |wm| wm.interface,
            ),
        };
        if wm_process.is_none() && arg_value(args, "--wm-interface").is_some() {
            return Err("--wm-interface=sophia_wm_v1 requires --wm-process".into());
        }
        let explicit_shell_process = arg_value(args, "--shell-process");
        let default_shell_process = arg_value(args, "--shell-process-default");
        for process in [&explicit_shell_process, &default_shell_process].into_iter().flatten() {
            if !std::path::Path::new(process).is_absolute() {
                return Err("shell process selections require an absolute path".into());
            }
        }
        let profile_is_compiled_default = desktop_profile_source.is_none();
        let resolved_shell_process = || -> Option<String> {
            explicit_shell_process.clone()
                .or_else(|| components.shell_client.as_ref().map(|p| p.to_string_lossy().into_owned()))
                .or_else(|| default_shell_process.clone()).or_else(|| {
                wm_process.as_ref().and_then(|process| {
                    let process = std::path::Path::new(process);
                    process.is_absolute().then(|| {
                        let parent = process
                            .parent()
                            .expect("an absolute executable has a parent");
                        parent.join("narthex").to_string_lossy().into_owned()
                    })
                })
            })
        };
        // The compiled default profile enables a shell, because it describes a
        // full desktop. A session running one application has no shell process
        // and no window manager to infer one beside, and refusing on that made
        // every such session unstartable. A default asking for a desktop this
        // session is not gets the shell turned off and reported; an explicit
        // profile still refuses, because someone wrote that intent down.
        let shell_dropped = shell_enabled
            && normal_session
            && profile_is_compiled_default
            && (wm_interface != sophia_config::ExternalWmInterface::SophiaWmV1
                || resolved_shell_process().is_none());
        let live_shell_enabled = shell_enabled && normal_session && !shell_dropped;
        let shell_process = if live_shell_enabled {
            if wm_interface != sophia_config::ExternalWmInterface::SophiaWmV1 {
                return Err("an enabled shell requires --wm-interface=sophia_wm_v1".into());
            }
            let process = resolved_shell_process()
                .ok_or("an enabled shell requires --shell-process or an absolute WM path")?;
            Some(process)
        } else {
            if explicit_shell_process.is_some() || components.shell_client.is_some() {
                return Err(if shell_enabled {
                    "--shell-process requires --session-mode=normal"
                } else {
                    "--shell-process requires shell { enabled #true; }"
                }
                .into());
            }
            None
        };
        let shell_config = std::env::var_os("SOPHIA_SHELL_CONFIG")
            .map(std::path::PathBuf::from)
            .or_else(|| components.shell_config.clone())
            .or_else(|| {
                // Preserve the installed Narthex default without disclosing
                // its private settings to an explicitly selected replacement.
                (shell_process.as_ref().is_some_and(|process| std::path::Path::new(process).file_name().is_some_and(|name| name == "narthex"))
                    && explicit_shell_process.is_none()
                    && components.shell_client.is_none())
                    .then(|| user_config_root.as_ref().map(|root| root.join("narthex/config.kdl")))
                    .flatten().filter(|path| path.is_file())
            });
        if let Some(path) = &shell_config {
            if shell_process.is_none() {
                return Err("a private shell config requires an enabled shell".into());
            }
            if !path.is_absolute() || !path.is_file() {
                return Err("private shell config must be an absolute existing file".into());
            }
        }
        // A panel with no shell to draw it would reserve work area nothing
        // fills, so it is refused rather than dropped: a profile that asks for
        // a desktop it cannot get should say so at startup.
        let shell_panel_thickness =
            sophia_config::desktop_profile_shell_panel_thickness(&desktop_profile);
        if shell_panel_thickness.is_some() && shell_process.is_none() {
            return Err(if shell_enabled {
                "shell { panel } requires --session-mode=normal"
            } else {
                "shell { panel } requires shell { enabled #true; }"
            }
            .into());
        }
        let shell_content_enabled =
            sophia_config::desktop_profile_shell_content_enabled(&desktop_profile);
        let shell_content_input_enabled =
            sophia_config::desktop_profile_shell_content_input_enabled(&desktop_profile);
        let shell_gpu_mode = sophia_config::desktop_profile_shell_gpu_mode(&desktop_profile);
        if shell_content_enabled {
            if shell_process.is_none() {
                return Err("shell { content #true; } requires an enabled shell".into());
            }
            if shell_panel_thickness.is_none() {
                return Err("shell content requires a positive shell { panel } allowance".into());
            }
        }
        if shell_content_input_enabled && !shell_content_enabled {
            return Err("shell { content-input #true; } requires shell { content #true; }".into());
        }
        if shell_gpu_mode == sophia_config::ShellGpuMode::Direct && shell_process.is_none() {
            return Err("shell { gpu \"direct\"; } requires an enabled shell".into());
        }
        let shell_proof_restart_after_visible = arg_value(
            args,
            "--shell-proof-restart-after-visible",
        )
        .map(|value| parse_u64(&value))
        .transpose()?
        .map(|value| u32::try_from(value).map_err(|_| "shell proof count is too large"))
        .transpose()?;
        if shell_proof_restart_after_visible.is_some_and(|count| count == 0 || count > 16) {
            return Err("--shell-proof-restart-after-visible must be 1-16".into());
        }
        if shell_proof_restart_after_visible.is_some() && shell_process.is_none() {
            return Err("--shell-proof-restart-after-visible requires an enabled shell".into());
        }
        // Every normal session, not only one running a window manager. The
        // `wm_process.is_some()` this replaces meant a session without one was
        // never told that a binding it carried could do nothing -- which is
        // most single-application proofs, and now the standalone and native
        // profiles too.
        let dropped_shortcuts = if normal_session {
            applications.validate_shortcuts(
                &shortcut_profile_candidate,
                live_shell_enabled,
                profile_is_compiled_default,
            )?
        } else {
            Vec::new()
        };
        let (wm_public_fault_after, wm_public_restart_after_action) =
            wm_proof::parse_wm_proof_controls(args, wm_process.is_some(), max_runtime)?;
        let output_proof_rollback_after_apply = parse_output_proof_rollback_after_apply(
            args,
            native_scanout,
            normal_session,
            wm_process.is_some(),
            max_runtime,
            wm_public_fault_after.is_some() || wm_public_restart_after_action.is_some(),
        )?;
        if native_scanout && std::env::var_os("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE").is_none() {
            return Err(
                "set SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 to run persistent native scanout"
                    .into(),
            );
        }
        let no_input = args.iter().any(|argument| argument == "--no-input");
        let input_devices_argument = arg_value(args, "--input-devices");
        let input_seat_argument = arg_value(args, "--input-seat");
        if no_input && (native_scanout || input_devices_argument.is_some() || input_seat_argument.is_some()) {
            return Err("--no-input requires a non-native session without input overrides".into());
        }
        if input_devices_argument.is_some() && input_seat_argument.is_some() {
            return Err("--input-seat and --input-devices are mutually exclusive".into());
        }
        let input_devices = input_devices_argument
            .map(|paths| {
                paths
                    .split(',')
                    .map(std::path::PathBuf::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                if no_input || input_seat_argument.is_some() {
                    Vec::new()
                } else {
                    match &core_snapshot.input.source {
                        sophia_config::InputSourceConfig::Devices(devices) => devices.clone(),
                        sophia_config::InputSourceConfig::Seat(_) => Vec::new(),
                    }
                }
            });
        if input_devices.len() > 16
            || input_devices
                .iter()
                .any(|path| !path.is_absolute() || path.as_os_str().is_empty())
        {
            return Err("--input-devices accepts 1-16 comma-separated absolute paths".into());
        }
        let input_seat = input_seat_argument.or_else(|| {
            if !no_input && input_devices.is_empty() {
                match &core_snapshot.input.source {
                    sophia_config::InputSourceConfig::Seat(seat) => Some(seat.clone()),
                    sophia_config::InputSourceConfig::Devices(_) => None,
                }
            } else {
                None
            }
        });
        if input_seat.as_ref().is_some_and(|seat| {
            seat.is_empty()
                || seat.len() > 64
                || !seat
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        }) {
            return Err("--input-seat accepts a 1-64 byte ASCII seat name".into());
        }
        if input_seat.is_some() && !input_devices.is_empty() {
            return Err("--input-seat and --input-devices are mutually exclusive".into());
        }
        if inject_text.is_some() && expect_physical_text.is_some() {
            return Err("--inject-text and --expect-physical-text are mutually exclusive".into());
        }
        if terminal_exec.is_some() && (inject_text.is_some() || expect_physical_text.is_some()) {
            return Err("--terminal-exec cannot be combined with input-proof commands".into());
        }
        if client.is_some()
            && (terminal_exec.is_some()
                || secondary_terminal
                || args.iter().any(|arg| arg.starts_with("--terminal=")))
        {
            return Err(
                "--client cannot be combined with terminal-specific session options".into(),
            );
        }
        if client.is_some() && inject_text.is_some() && !proof_mode {
            return Err("--client with --inject-text requires explicit --proof mode".into());
        }
        if client.is_some() && inject_text.is_some() && expect_client_stdout.is_none() {
            return Err("--client with --inject-text requires --expect-client-stdout".into());
        }
        if asked_atomic_cursor && legacy_cursor {
            return Err("--atomic-cursor and --legacy-cursor are mutually exclusive".into());
        }
        // The flags are about which hardware path a session prefers, and a
        // session without native scanout has neither. Asked for explicitly,
        // that is a contradiction worth naming; the default simply does not
        // apply -- `use_atomic_cursor_plane` is only ever called by a native
        // session.
        if (asked_atomic_cursor || legacy_cursor) && (!native_scanout || !normal_session) {
            return Err(
                "--atomic-cursor and --legacy-cursor require --native-scanout and --session-mode=normal"
                    .into(),
            );
        }
        if direct_cursor_proof && (!native_scanout || !normal_session) {
            return Err(
                "--direct-cursor-proof requires --native-scanout and --session-mode=normal".into(),
            );
        }
        if direct_overlay_proof && (!native_scanout || !normal_session) {
            return Err(
                "--direct-overlay-proof requires --native-scanout and --session-mode=normal".into(),
            );
        }
        if direct_overlay_hold_ticks != 0 && !direct_overlay_proof {
            return Err("--direct-overlay-hold-ticks requires --direct-overlay-proof".into());
        }
        if (m4_first_acquire_delay.is_some()
            || m4_reject_first_present
            || m4_diagnose_first_mixed_export)
            && (!native_scanout || terminal_exec.is_none())
        {
            return Err(
                "M4 Present proof controls require --native-scanout and --terminal-exec".into(),
            );
        }
        if (inject_text.is_some() || expect_physical_text.is_some())
            && max_runtime.is_none()
            && max_ticks.is_none()
        {
            return Err(
                "input proof flags require --max-runtime-ms or --max-ticks for a bounded proof"
                    .into(),
            );
        }
        if expect_physical_text.is_some() && input_devices.is_empty() && input_seat.is_none() {
            return Err("--expect-physical-text requires --input-seat or --input-devices".into());
        }
        if expect_physical_pointer && expect_physical_text.is_none() {
            return Err(
                "--expect-physical-pointer requires --expect-physical-text for visible content"
                    .into(),
            );
        }
        if exit_after_input_proof && inject_text.is_none() && expect_physical_text.is_none() {
            return Err("--exit-after-input-proof requires an input proof".into());
        }
        if let Some(text) = inject_text.as_ref().or(expect_physical_text.as_ref())
            && (text.is_empty()
                || text.len() > 24
                || !text.bytes().all(|byte| byte.is_ascii_lowercase()))
        {
            return Err("input proof text accepts 1-24 lowercase ASCII letters".into());
        }
        Ok(Self {
            display,
            socket_path: std::path::PathBuf::from(format!("/tmp/.X11-unix/X{display_number}")),
            terminal: arg_value(args, "--terminal").unwrap_or_else(|| "xterm".to_owned()),
            terminal_exec,
            terminal_exec_args,
            session_launcher,
            session_browser,
            client,
            client_args,
            expect_client_stdout,
            require_client_normal_exit,
            secondary_terminal,
            max_runtime,
            normal_session,
            exit_when_startup_exits,
            startup_ready_timeout,
            applications,
            session_application_overrides,
            control_access: session_profile.candidate().control,
            control_socket: None,
            application_catalog,
            session_profile,
            active_launch_profile: None,
            max_ticks,
            inject_text,
            expect_physical_text,
            physical_sequence_timeout_msec: physical_sequence_timeout
                .unwrap_or(SESSION_PHYSICAL_SEQUENCE_TIMEOUT_MSEC),
            expect_physical_pointer,
            exit_after_input_proof,
            input_devices,
            input_seat,
            native_scanout,
            software_client_rendering,
            wm_process,
            wm_process_args,
            wm_process_executable_grants,
            shell_process,
            shell_config,
            shell_panel_thickness,
            shell_content_enabled,
            shell_content_input_enabled,
            shell_gpu_mode,
            shell_proof_restart_after_visible,
            wm_interface,
            wm_public_fault_after,
            wm_public_restart_after_action,
            output_proof_rollback_after_apply,
            wm_socket_path: std::env::temp_dir().join(format!(
                "sophia-live-wm-{}-{display_number}.sock",
                std::process::id()
            )),
            input_quiet_msec: SESSION_INPUT_QUIET_MSEC,
            namespace_profile,

            namespace_capabilities: NamespaceCapabilities::NONE,
            xkb_config,
            key_repeat_config,
            initial_caps_lock,
            initial_num_lock,
            shortcut_profile_candidate,
            shell_dropped,
            dropped_shortcuts,
            input_profile,
            output_profile,
            desktop_profile,
            desktop_profile_source,
            desktop_profile_activation: sophia_config::DesktopProfileActivationModel::default(),
            surface_chrome_style: Self::surface_chrome_style(core_snapshot.fallback_chrome),
            cursor_resolution,
            cursor_shake_resolution,
            verbose_diagnostics: core_snapshot.verbose_diagnostics,
            core_config_source,
            core_config_state,
            inject_output_size,
            inject_surface_resize,
            inject_surface_resize_sequence,
            m4_first_acquire_delay,
            m4_reject_first_present,
            atomic_cursor,
            direct_cursor_proof,
            direct_overlay_proof,
            direct_overlay_hold_ticks,
            m4_diagnose_first_mixed_export,
            firefox_m8_proof,
            firefox_m10_proof,
            firefox_m10_rendering_proof,
            firefox_m10_dialog_proof,
            firefox_m10_primary_proof,
            firefox_m10_selection_proof,
            firefox_m10_lifecycle_proof,
        })
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
    ) -> Result<Child, Box<dyn std::error::Error>> {
        let mut command = std::process::Command::new(&app.executable);
        configure_control_environment(&mut command, control_socket);
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
        Ok(command.spawn()?)
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
