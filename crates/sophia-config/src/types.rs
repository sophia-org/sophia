use std::fmt;
use std::path::PathBuf;

pub const SOPHIA_CONFIG_SCHEMA_VERSION: u32 = 2;
pub const SOPHIA_CONFIG_MAX_BYTES: usize = 1024 * 1024;
pub const SOPHIA_CONFIG_MAX_APPLICATIONS: usize = 32;
pub const SOPHIA_CONFIG_MAX_ARGUMENTS: usize = 32;
pub const SOPHIA_CONFIG_MAX_ARGUMENT_BYTES: usize = 4_096;
pub const SOPHIA_CONFIG_MAX_OUTPUTS: usize = 32;
pub const SOPHIA_CONFIG_MAX_WM_ACTIONS: usize = 256;
pub const SOPHIA_CONFIG_MAX_WM_BINDINGS: usize = 256;
pub const SOPHIA_CONFIG_MAX_WORKSPACES: usize = 64;
pub const SOPHIA_CONFIG_COMPILED_MAX_CHROME_WIDTH: u32 = 64;
pub const SOPHIA_CONFIG_MAX_CURSOR_NAME_BYTES: usize = 64;
pub const SOPHIA_CONFIG_MAX_CURSOR_SIZE: u32 = 128;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct ConfigGeneration(u64);

impl ConfigGeneration {
    pub const INITIAL: Self = Self(1);

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ConfigDigest([u8; 32]);

impl ConfigDigest {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Debug for ConfigDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ConfigDigest({self})")
    }
}

impl fmt::Display for ConfigDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReloadDisposition {
    Applied,
    Deferred,
    PendingRestart,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusRingStyle {
    pub enabled: bool,
    pub width: u32,
    pub color: Rgb8,
}

impl Default for FocusRingStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 2,
            color: Rgb8 {
                red: 0x70,
                green: 0xb7,
                blue: 0xff,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameStyle {
    pub enabled: bool,
    pub width: u32,
    pub focused_color: Rgb8,
    pub unfocused_color: Rgb8,
}

impl Default for FrameStyle {
    fn default() -> Self {
        Self {
            enabled: false,
            width: 0,
            focused_color: FocusRingStyle::default().color,
            unfocused_color: Rgb8 {
                red: 0x30,
                green: 0x30,
                blue: 0x30,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChromePolicy {
    pub focus_ring: FocusRingStyle,
    pub frame: FrameStyle,
}

impl ChromePolicy {
    pub const fn clearance(self) -> u32 {
        let ring = if self.focus_ring.enabled {
            self.focus_ring.width
        } else {
            0
        };
        let frame = if self.frame.enabled {
            self.frame.width
        } else {
            0
        };
        if ring > frame { ring } else { frame }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorConfig {
    pub theme: String,
    pub size: u32,
    pub shape: String,
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            theme: "x11-core".to_owned(),
            size: 16,
            shape: "left_ptr".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationConfig {
    pub id: u64,
    pub name: String,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    /// Optional opaque class emitted once for the first surface observed from
    /// this trusted registered launch.
    pub placement_classification: Option<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionConfig {
    pub applications: Vec<ApplicationConfig>,
    pub startup: Vec<u64>,
    pub application_catalogs: Vec<ApplicationCatalogConfig>,
}

/// Operator-owned launch policy. Desktop entries supply commands, never policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationCatalogConfig {
    pub name: String,
    pub sources: Vec<PathBuf>,
    pub applications: Vec<String>,
    pub terminal: Option<String>,
    pub terminal_arguments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputSourceConfig {
    Seat(String),
    Devices(Vec<PathBuf>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XkbConfig {
    pub rules: String,
    pub model: String,
    pub layout: String,
    pub variant: String,
    pub options: String,
}

impl Default for XkbConfig {
    fn default() -> Self {
        Self {
            rules: "evdev".to_owned(),
            model: "pc105".to_owned(),
            layout: "us".to_owned(),
            variant: String::new(),
            options: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepeatConfig {
    pub delay_msec: u64,
    pub interval_msec: u64,
}

impl Default for RepeatConfig {
    fn default() -> Self {
        Self {
            delay_msec: 660,
            interval_msec: 25,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputConfig {
    pub source: InputSourceConfig,
    pub xkb: XkbConfig,
    pub repeat: RepeatConfig,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            source: InputSourceConfig::Seat("seat0".to_owned()),
            xkb: XkbConfig::default(),
            repeat: RepeatConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputConfig {
    pub identity: String,
    pub x: i32,
    pub y: i32,
    pub mode: String,
    pub scale: u32,
    pub primary: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExternalWmInterface {
    #[default]
    SophiaWmV1,
}

impl ExternalWmInterface {
    pub const fn name(self) -> &'static str {
        match self {
            Self::SophiaWmV1 => "sophia_wm_v1",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalWmConfig {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub interface: ExternalWmInterface,
}

/// Session-owned launch selections; never part of the WM's policy fragment.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DesktopComponents {
    pub window_manager: Option<ExternalWmConfig>,
    pub shell_client: Option<PathBuf>,
    pub shell_config: Option<PathBuf>,
    pub shell_components: Vec<crate::ShellComponentConfig>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreConfigSnapshot {
    pub schema: u32,
    pub generation: ConfigGeneration,
    pub digest: ConfigDigest,
    pub session: SessionConfig,
    pub input: InputConfig,
    pub outputs: Vec<OutputConfig>,
    pub fallback_chrome: ChromePolicy,
    pub max_chrome_width: u32,
    pub cursor: CursorConfig,
    pub namespace_profile: String,
    pub external_wm: Option<ExternalWmConfig>,
    pub verbose_diagnostics: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmActionBehavior {
    FocusNext,
    FocusPrevious,
    NextLayout,
    ActivateWorkspace { workspace: u64 },
    LaunchApplication { application: u64 },
    CloseFocused,
    Logout,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmActionConfig {
    pub id: u64,
    pub name: String,
    pub behavior: WmActionBehavior,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmBindingConfig {
    pub action: u64,
    pub keycode: u32,
    pub modifiers: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmLayoutKind {
    Columns,
    Natural,
}

impl WmLayoutKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Columns => "columns",
            Self::Natural => "natural",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmConfigSnapshot {
    pub schema: u32,
    pub generation: ConfigGeneration,
    pub digest: ConfigDigest,
    pub timeout_msec: u32,
    pub workspaces: Vec<u64>,
    pub layout: WmLayoutKind,
    pub actions: Vec<WmActionConfig>,
    pub bindings: Vec<WmBindingConfig>,
    pub chrome: ChromePolicy,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreConfigDelta {
    pub applications_changed: bool,
    pub repeat_changed: bool,
    pub chrome_changed: bool,
    pub cursor_changed: bool,
    pub diagnostics_changed: bool,
    pub restart_required: bool,
}

impl CoreConfigDelta {
    pub fn between(active: &CoreConfigSnapshot, candidate: &CoreConfigSnapshot) -> Self {
        Self {
            applications_changed: active.session != candidate.session,
            repeat_changed: active.input.repeat != candidate.input.repeat,
            chrome_changed: active.fallback_chrome != candidate.fallback_chrome
                || active.max_chrome_width != candidate.max_chrome_width,
            cursor_changed: active.cursor != candidate.cursor,
            diagnostics_changed: active.verbose_diagnostics != candidate.verbose_diagnostics,
            restart_required: active.input.source != candidate.input.source
                || active.input.xkb != candidate.input.xkb
                || active.outputs != candidate.outputs
                || active.namespace_profile != candidate.namespace_profile
                || active.external_wm != candidate.external_wm,
        }
    }

    pub const fn is_empty(self) -> bool {
        !self.applications_changed
            && !self.repeat_changed
            && !self.chrome_changed
            && !self.cursor_changed
            && !self.diagnostics_changed
            && !self.restart_required
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WmConfigDelta {
    pub bindings_changed: bool,
    pub policy_changed: bool,
    pub chrome_changed: bool,
}

impl WmConfigDelta {
    pub fn between(active: &WmConfigSnapshot, candidate: &WmConfigSnapshot) -> Self {
        Self {
            bindings_changed: active.bindings != candidate.bindings,
            policy_changed: active.timeout_msec != candidate.timeout_msec
                || active.workspaces != candidate.workspaces
                || active.layout != candidate.layout
                || active.actions != candidate.actions,
            chrome_changed: active.chrome != candidate.chrome,
        }
    }

    pub const fn is_empty(self) -> bool {
        !self.bindings_changed && !self.policy_changed && !self.chrome_changed
    }
}
