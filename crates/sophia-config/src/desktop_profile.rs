use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

mod application_commands;
mod application_source;
mod staging;

pub(crate) use application_commands::lower_application_commands;
pub use application_source::render_desktop_profile_source;
pub use staging::*;

use kdl::{KdlDocument, KdlNode};
use sha2::{Digest, Sha256};

use crate::{
    ConfigDigest, ConfigGeneration, ConfigIoError, ConfigParseError, DesktopProfileActivationKey,
    read_config_file,
};

pub const DESKTOP_PROFILE_MAX_DEPTH: usize = 10;

/// The largest panel a profile may ask for, in pixels.
///
/// This crate depends on nothing else in the stack, so it cannot read the
/// wire's own ceiling. The value is `sophia_protocol`'s
/// `SOPHIA_SHELL_MAX_RESERVATION_THICKNESS_PX`, and a test in `sophia-cli` --
/// which sees both crates -- fails if they ever drift apart.
pub const SHELL_PANEL_MAX_THICKNESS_PX: u16 = 512;
/// First admitted Lom GPU-domain ceiling. A different value needs a new
/// measured GPU admission decision rather than an implicit local default.
pub const SHELL_GPU_MEMORY_BYTES: u64 = 256 * 1024 * 1024;

pub const DESKTOP_PROFILE_MAX_FILES: usize = 64;
pub const DESKTOP_PROFILE_MAX_BYTES: usize = 1024 * 1024;

pub const COMPILED_DESKTOP_PROFILE: &str = r#"schema 1
policy {
  layout "scroller"
  layout-cycle "scroller" "tile" "grid" "monocle" "vertical-scroller"
  view-count 9
  gaps 0
}
shell { enabled #true; }
shortcut {
  profile "compiled"
  bind "Ctrl+Alt+Delete" "session:logout"
  bind "Super+Return" "session:spawn-terminal"
  bind "Super+b" "session:spawn-browser"
  bind "Super+p" "session:window-switcher"
  bind "Super+q" "session:close-window"
  bind "Super+h" "policy:focus-prev"
  bind "Super+j" "policy:focus-next"
  bind "Super+k" "policy:focus-prev"
  bind "Super+l" "policy:focus-next"
  bind "Super+Shift+Tab" "policy:focus-prev"
  bind "Super+Left" "policy:focus-output-next"
  bind "Super+Right" "policy:focus-output-prev"
  bind "Super+Shift+Left" "policy:move-to-output-prev"
  bind "Super+Shift+Right" "policy:move-to-output-next"
  bind "Super+u" "policy:focus-occupied-workspace-next"
  bind "Super+Shift+u" "policy:focus-view-next"
  bind "Super+1" "policy:focus-workspace 1"
  bind "Super+2" "policy:focus-workspace 2"
  bind "Super+3" "policy:focus-workspace 3"
  bind "Super+4" "policy:focus-workspace 4"
  bind "Super+5" "policy:focus-workspace 5"
  bind "Super+6" "policy:focus-workspace 6"
  bind "Super+7" "policy:focus-workspace 7"
  bind "Super+8" "policy:focus-workspace 8"
  bind "Super+9" "policy:focus-workspace 9"
  bind "Super+Ctrl+1" "policy:move-to-workspace 1"
  bind "Super+Ctrl+2" "policy:move-to-workspace 2"
  bind "Super+Ctrl+3" "policy:move-to-workspace 3"
  bind "Super+Ctrl+4" "policy:move-to-workspace 4"
  bind "Super+Ctrl+5" "policy:move-to-workspace 5"
  bind "Super+Ctrl+6" "policy:move-to-workspace 6"
  bind "Super+Ctrl+7" "policy:move-to-workspace 7"
  bind "Super+Ctrl+8" "policy:move-to-workspace 8"
  bind "Super+Ctrl+9" "policy:move-to-workspace 9"
  bind "Super+f" "policy:toggle-maximized"
  bind "Super+Shift+f" "policy:toggle-fullscreen"
  bind "Super+m" "policy:toggle-maximized"
  bind "Super+t" "policy:toggle-floating"
  bind "Super+Shift+t" "policy:toggle-floating"
  bind "Super+Shift+b" "policy:minimize"
  bind "Super+Alt+b" "policy:restore-minimized"
  bind "Super+s" "policy:move-to-scratchpad"
  bind "Super+Alt+s" "policy:toggle-scratchpad"
  bind "Super+Shift+s" "policy:restore-scratchpad"
  bind "Super+n" "policy:switch-layout"
  bind "Super+Shift+z" "policy:consume-window"
  bind "Super+Ctrl+z" "policy:expel-window"
  bind "Super+-" "policy:resize-width -0.1"
  bind "Super+=" "policy:resize-width 0.1"
  pointer-bind "Super+left" "policy:move"
  pointer-bind "Super+right" "policy:resize"
}
session { terminal "terminal"; browser "browser"; }
input { inherit-sophia #true; }
output { inherit-sophia #true; }
broker { enabled #false; }
"#;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DesktopAuthority {
    Policy,
    Shell,
    Shortcut,
    Session,
    Input,
    Output,
    Broker,
}

impl DesktopAuthority {
    pub const ALL: [Self; 7] = [
        Self::Policy,
        Self::Shell,
        Self::Shortcut,
        Self::Session,
        Self::Input,
        Self::Output,
        Self::Broker,
    ];

    /// Startup activates the external policy authority last. Once Hagia
    /// acknowledges activation it may emit normal policy traffic, so every
    /// Sophia-owned participant must already be active.
    pub const STARTUP_ACTIVATION_ORDER: [Self; 7] = [
        Self::Shell,
        Self::Shortcut,
        Self::Session,
        Self::Input,
        Self::Output,
        Self::Broker,
        Self::Policy,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Shell => "shell",
            Self::Shortcut => "shortcut",
            Self::Session => "session",
            Self::Input => "input",
            Self::Output => "output",
            Self::Broker => "broker",
        }
    }

    fn parse(name: &str) -> Result<Self, DesktopProfileError> {
        Self::ALL
            .into_iter()
            .find(|authority| authority.name() == name)
            .ok_or_else(|| DesktopProfileError::Schema(format!("unsupported section {name:?}")))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopValueProvenance {
    pub path: PathBuf,
    pub ordinal: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopProfileValue {
    /// Policy labels describe records, not unique WM setting identities.
    /// Repeated policy labels retain their order; only the WM resolves them.
    pub key: String,
    pub encoded: String,
    pub provenance: DesktopValueProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopAuthorityCandidate {
    pub authority: DesktopAuthority,
    pub generation: ConfigGeneration,
    pub digest: ConfigDigest,
    pub values: Vec<DesktopProfileValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopProfileGeneration {
    pub generation: ConfigGeneration,
    pub digest: ConfigDigest,
    pub sources: Vec<PathBuf>,
    pub candidates: BTreeMap<DesktopAuthority, DesktopAuthorityCandidate>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedDesktopProfileCandidates {
    pub shortcut: crate::DesktopShortcutCandidate,
    pub session: crate::DesktopSessionCandidate,
    pub input: crate::DesktopInputCandidate,
    pub output: crate::DesktopOutputCandidate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedDesktopProfile {
    pub profile: DesktopProfileGeneration,
    pub candidates: PreparedDesktopProfileCandidates,
}

impl PreparedDesktopProfile {
    pub const fn activation_key(&self) -> crate::DesktopProfileActivationKey {
        crate::DesktopProfileActivationKey::new(self.profile.generation, self.profile.digest)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DesktopProfileError {
    Io(ConfigIoError),
    Parse(ConfigParseError),
    Schema(String),
    Limit(String),
    Stage(String),
}

impl fmt::Display for DesktopProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Parse(error) => error.fmt(formatter),
            Self::Schema(error) => write!(formatter, "desktop profile schema error: {error}"),
            Self::Limit(error) => write!(formatter, "desktop profile limit exceeded: {error}"),
            Self::Stage(error) => write!(formatter, "desktop profile staging failed: {error}"),
        }
    }
}

impl std::error::Error for DesktopProfileError {}

impl From<ConfigIoError> for DesktopProfileError {
    fn from(error: ConfigIoError) -> Self {
        Self::Io(error)
    }
}

impl From<ConfigParseError> for DesktopProfileError {
    fn from(error: ConfigParseError) -> Self {
        Self::Parse(error)
    }
}

pub fn discover_desktop_profile_source(
    explicit: Option<&Path>,
    xdg_config_home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    if let Some(root) = xdg_config_home {
        for relative in ["sophia/desktop.kdl", "hagia/config.kdl"] {
            let path = root.join(relative);
            if path.symlink_metadata().is_ok() {
                return Some(path);
            }
        }
    }
    ["/etc/sophia/desktop.kdl", "/etc/hagia/config.kdl"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.symlink_metadata().is_ok())
}

pub fn load_desktop_profile(
    source: Option<&Path>,
    generation: ConfigGeneration,
) -> Result<DesktopProfileGeneration, DesktopProfileError> {
    Ok(load_prepared_desktop_profile(source, generation)?.profile)
}

pub fn load_prepared_desktop_profile(
    source: Option<&Path>,
    generation: ConfigGeneration,
) -> Result<PreparedDesktopProfile, DesktopProfileError> {
    if generation.raw() == 0 {
        return Err(DesktopProfileError::Schema(
            "generation must be nonzero".to_owned(),
        ));
    }
    let mut expansion = Expansion::default();
    let nodes = if let Some(source) = source {
        expansion.expand(source, 0)?
    } else {
        expansion.sources.push(PathBuf::from("<compiled>"));
        expansion.digest_input.extend_from_slice(b"<compiled>\0");
        expansion
            .digest_input
            .extend_from_slice(COMPILED_DESKTOP_PROFILE.as_bytes());
        parse_nodes(COMPILED_DESKTOP_PROFILE, Path::new("<compiled>"))?
    };
    let digest = ConfigDigest::new(Sha256::digest(&expansion.digest_input).into());
    let candidates = partition(&nodes, generation, digest)?;
    let profile = DesktopProfileGeneration {
        generation,
        digest,
        sources: expansion.sources,
        candidates,
    };
    let profile = lower_application_commands(&profile)?;
    let candidates = prepare_desktop_profile_candidates(&profile)?;
    Ok(PreparedDesktopProfile {
        profile,
        candidates,
    })
}

pub fn load_desktop_authority_fragment(
    path: &Path,
    expected_authority: DesktopAuthority,
    expected_key: crate::DesktopProfileActivationKey,
) -> Result<DesktopAuthorityCandidate, DesktopProfileError> {
    if !path.is_absolute() {
        return Err(ConfigIoError::InvalidPath.into());
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|error| ConfigIoError::Metadata(error.to_string()))?;
    if metadata.file_type().is_symlink() {
        return Err(ConfigIoError::NotRegularFile.into());
    }
    let canonical =
        fs::canonicalize(path).map_err(|error| ConfigIoError::Metadata(error.to_string()))?;
    let bytes = read_config_file(&canonical)?;
    let source = std::str::from_utf8(&bytes).map_err(|_| ConfigParseError::NotUtf8)?;
    let document = KdlDocument::parse_v2(source)
        .map_err(|error| ConfigParseError::Syntax(error.to_string()))?;
    let mut schema_seen = false;
    let mut generation = None;
    let mut digest = None;
    let mut authority_seen = false;
    let mut keys = BTreeSet::new();
    let mut values = Vec::new();
    for (ordinal, node) in document.nodes().iter().enumerate() {
        match node.name().value() {
            "schema" => {
                if schema_seen || integer_argument(node) != Some(1) || node.children().is_some() {
                    return Err(fragment_schema("requires exactly one schema 1 declaration"));
                }
                schema_seen = true;
            }
            "profile-generation" => {
                let raw = exact_integer_argument(node, "authority fragment generation")?;
                let raw = u64::try_from(raw)
                    .ok()
                    .filter(|raw| *raw != 0)
                    .ok_or_else(|| fragment_schema("generation must be nonzero u64"))?;
                if generation
                    .replace(ConfigGeneration::from_raw(raw))
                    .is_some()
                {
                    return Err(fragment_schema("duplicate generation"));
                }
            }
            "profile-digest" => {
                let parsed = parse_fragment_digest(exact_string_argument(
                    node,
                    "authority fragment digest",
                )?)?;
                if digest.replace(parsed).is_some() {
                    return Err(fragment_schema("duplicate digest"));
                }
            }
            name => {
                if authority_seen {
                    return Err(fragment_schema("contains more than one authority section"));
                }
                let authority = DesktopAuthority::parse(name)?;
                if authority != expected_authority {
                    return Err(fragment_schema("crossed its assigned authority boundary"));
                }
                if !node.entries().is_empty() || node.ty().is_some() {
                    return Err(fragment_schema("authority section has an ambiguous shape"));
                }
                let children = node
                    .children()
                    .ok_or_else(|| fragment_schema("authority section requires children"))?;
                authority_seen = true;
                for child in children.nodes() {
                    validate_setting(authority, child)?;
                    let key = setting_key(authority, child)?;
                    if authority != DesktopAuthority::Policy && !keys.insert(key.clone()) {
                        return Err(fragment_schema("contains a duplicate setting"));
                    }
                    values.push(DesktopProfileValue {
                        key,
                        encoded: child.to_string().trim().to_owned(),
                        provenance: DesktopValueProvenance {
                            path: canonical.clone(),
                            ordinal: ordinal + 1,
                        },
                    });
                }
            }
        }
    }
    let generation = generation.ok_or_else(|| fragment_schema("missing generation"))?;
    let digest = digest.ok_or_else(|| fragment_schema("missing digest"))?;
    if !schema_seen || !authority_seen {
        return Err(fragment_schema("is incomplete"));
    }
    if generation != expected_key.generation() || digest != expected_key.digest() {
        return Err(fragment_schema(
            "identity does not match the activation key",
        ));
    }
    Ok(DesktopAuthorityCandidate {
        authority: expected_authority,
        generation,
        digest,
        values,
    })
}

pub fn validate_desktop_profile_fragments(
    fragments: &DesktopProfileFragments,
    expected_key: DesktopProfileActivationKey,
) -> Result<(), DesktopProfileError> {
    if fragments.generation != expected_key.generation()
        || fragments.digest != expected_key.digest()
    {
        return Err(fragment_schema(
            "set identity does not match the activation key",
        ));
    }
    for authority in DesktopAuthority::ALL {
        load_desktop_authority_fragment(fragments.path(authority), authority, expected_key)?;
    }
    Ok(())
}

fn fragment_schema(message: &str) -> DesktopProfileError {
    DesktopProfileError::Schema(format!("authority fragment {message}"))
}

fn parse_fragment_digest(value: &str) -> Result<ConfigDigest, DesktopProfileError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(fragment_schema("digest must be 64 hexadecimal characters"));
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| fragment_schema("digest must be hexadecimal"))?;
    }
    Ok(ConfigDigest::new(bytes))
}

pub fn prepare_desktop_profile_candidates(
    profile: &DesktopProfileGeneration,
) -> Result<PreparedDesktopProfileCandidates, DesktopProfileError> {
    for authority in DesktopAuthority::ALL {
        let candidate = profile.candidates.get(&authority).ok_or_else(|| {
            DesktopProfileError::Schema(format!("missing {} authority candidate", authority.name()))
        })?;
        if candidate.authority != authority
            || candidate.generation != profile.generation
            || candidate.digest != profile.digest
        {
            return Err(DesktopProfileError::Schema(format!(
                "inconsistent {} candidate identity",
                authority.name()
            )));
        }
    }
    let profile = lower_application_commands(profile)?;
    let candidate = |authority| {
        profile
            .candidates
            .get(&authority)
            .expect("all desktop authority candidates validated")
    };
    let session = crate::prepare_desktop_session_candidate(candidate(DesktopAuthority::Session))?;
    if !desktop_profile_shell_enabled(&profile)
        && (session.components.shell_client.is_some() || session.components.shell_config.is_some())
    {
        return Err(DesktopProfileError::Schema(
            "shell component selections require shell { enabled #true; }".to_owned(),
        ));
    }
    let shortcut =
        crate::prepare_desktop_shortcut_candidate(candidate(DesktopAuthority::Shortcut))?;
    for binding in &shortcut.bindings {
        if let crate::DesktopShortcutTarget::LaunchApplication(name) = &binding.target
            && !session
                .applications
                .iter()
                .any(|application| &application.name == name)
        {
            return Err(DesktopProfileError::Schema(format!(
                "shortcut references undeclared application {name:?}"
            )));
        }
    }
    Ok(PreparedDesktopProfileCandidates {
        shortcut,
        session,
        input: crate::prepare_desktop_input_candidate(candidate(DesktopAuthority::Input))?,
        output: crate::prepare_desktop_output_candidate(candidate(DesktopAuthority::Output))?,
    })
}

#[derive(Default)]
struct Expansion {
    files: usize,
    bytes: usize,
    stack: Vec<PathBuf>,
    seen: BTreeSet<PathBuf>,
    sources: Vec<PathBuf>,
    digest_input: Vec<u8>,
}

#[derive(Clone)]
struct ExpandedNode {
    node: KdlNode,
    provenance: DesktopValueProvenance,
}

impl Expansion {
    fn expand(
        &mut self,
        path: &Path,
        depth: usize,
    ) -> Result<Vec<ExpandedNode>, DesktopProfileError> {
        if depth > DESKTOP_PROFILE_MAX_DEPTH {
            return Err(DesktopProfileError::Limit(
                "include depth exceeds 10".to_owned(),
            ));
        }
        if !path.is_absolute() {
            return Err(ConfigIoError::InvalidPath.into());
        }
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| ConfigIoError::Metadata(error.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(ConfigIoError::NotRegularFile.into());
        }
        let canonical =
            fs::canonicalize(path).map_err(|error| ConfigIoError::Metadata(error.to_string()))?;
        if self.stack.contains(&canonical) {
            return Err(DesktopProfileError::Schema(format!(
                "include cycle reaches {}",
                canonical.display()
            )));
        }
        if !self.seen.insert(canonical.clone()) {
            return Err(DesktopProfileError::Schema(format!(
                "source included more than once: {}",
                canonical.display()
            )));
        }
        self.files += 1;
        if self.files > DESKTOP_PROFILE_MAX_FILES {
            return Err(DesktopProfileError::Limit("more than 64 files".to_owned()));
        }
        let bytes = read_config_file(&canonical)?;
        if bytes.is_empty() {
            return Err(DesktopProfileError::Schema("source is empty".to_owned()));
        }
        self.bytes = self.bytes.saturating_add(bytes.len());
        if self.bytes > DESKTOP_PROFILE_MAX_BYTES {
            return Err(DesktopProfileError::Limit(
                "aggregate exceeds one MiB".to_owned(),
            ));
        }
        let source = std::str::from_utf8(&bytes).map_err(|_| ConfigParseError::NotUtf8)?;
        let document = KdlDocument::parse_v2(source)
            .map_err(|error| ConfigParseError::Syntax(error.to_string()))?;
        self.stack.push(canonical.clone());
        self.sources.push(canonical.clone());
        self.digest_input
            .extend_from_slice(canonical.as_os_str().as_encoded_bytes());
        self.digest_input.push(0);
        self.digest_input.extend_from_slice(&bytes);
        self.digest_input.push(0);
        let mut result = Vec::new();
        for (index, node) in document.nodes().iter().enumerate() {
            if node.name().value() == "include" {
                let include = exact_string_argument(node, "include")?;
                let resolved = if Path::new(include).is_absolute() {
                    PathBuf::from(include)
                } else {
                    canonical
                        .parent()
                        .ok_or(ConfigIoError::InvalidPath)?
                        .join(include)
                };
                result.extend(self.expand(&resolved, depth + 1)?);
            } else {
                result.push(ExpandedNode {
                    node: node.clone(),
                    provenance: DesktopValueProvenance {
                        path: canonical.clone(),
                        ordinal: index + 1,
                    },
                });
            }
        }
        self.stack.pop();
        Ok(result)
    }
}

fn parse_nodes(source: &str, path: &Path) -> Result<Vec<ExpandedNode>, DesktopProfileError> {
    let document = KdlDocument::parse_v2(source)
        .map_err(|error| ConfigParseError::Syntax(error.to_string()))?;
    Ok(document
        .nodes()
        .iter()
        .enumerate()
        .map(|(index, node)| ExpandedNode {
            node: node.clone(),
            provenance: DesktopValueProvenance {
                path: path.to_path_buf(),
                ordinal: index + 1,
            },
        })
        .collect())
}

fn partition(
    nodes: &[ExpandedNode],
    generation: ConfigGeneration,
    digest: ConfigDigest,
) -> Result<BTreeMap<DesktopAuthority, DesktopAuthorityCandidate>, DesktopProfileError> {
    let mut candidates = DesktopAuthority::ALL
        .into_iter()
        .map(|authority| {
            (
                authority,
                DesktopAuthorityCandidate {
                    authority,
                    generation,
                    digest,
                    values: Vec::new(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut schema_seen = false;
    let mut keys = BTreeSet::new();
    for expanded in nodes {
        let node = &expanded.node;
        if node.name().value() == "schema" {
            if schema_seen || integer_argument(node) != Some(1) || node.children().is_some() {
                return Err(DesktopProfileError::Schema(
                    "requires exactly one schema 1 declaration".to_owned(),
                ));
            }
            schema_seen = true;
            continue;
        }
        if !node.entries().is_empty() || node.ty().is_some() {
            return Err(DesktopProfileError::Schema(format!(
                "ambiguous authority section {:?}",
                node.name().value()
            )));
        }
        let authority = DesktopAuthority::parse(node.name().value())?;
        let children = node.children().ok_or_else(|| {
            DesktopProfileError::Schema(format!(
                "authority section {:?} requires children",
                node.name().value()
            ))
        })?;
        for child in children.nodes() {
            validate_setting(authority, child)?;
            application_commands::validate_source_names(authority, child)?;
            let key = setting_key(authority, child)?;
            if authority != DesktopAuthority::Policy && !keys.insert(key.clone()) {
                return Err(DesktopProfileError::Schema(format!(
                    "duplicate setting {key:?}"
                )));
            }
            candidates
                .get_mut(&authority)
                .expect("all authority candidates exist")
                .values
                .push(DesktopProfileValue {
                    key,
                    encoded: child.to_string().trim().to_owned(),
                    provenance: expanded.provenance.clone(),
                });
        }
    }
    if !schema_seen {
        return Err(DesktopProfileError::Schema(
            "missing schema declaration".to_owned(),
        ));
    }
    Ok(candidates)
}

fn validate_setting(
    authority: DesktopAuthority,
    node: &KdlNode,
) -> Result<(), DesktopProfileError> {
    if node.ty().is_some() || node.entries().len() > 32 {
        return Err(DesktopProfileError::Schema(
            "typed or structurally excessive setting".to_owned(),
        ));
    }
    let name = node.name().value();
    if [
        "emergency-chord",
        "policy-timeout",
        "max-surfaces",
        "max-outputs",
        "renderer",
        "scanout",
        "namespace-profile",
    ]
    .contains(&name)
    {
        return Err(DesktopProfileError::Schema(
            "reserved control override".to_owned(),
        ));
    }
    let supported = match authority {
        // Policy is an ordered WM-owned payload. Its vocabulary and values
        // are admitted by the selected WM before profile activation.
        DesktopAuthority::Policy => true,
        DesktopAuthority::Shell => {
            ["enabled", "panel", "content", "gpu-memory-bytes"].contains(&name)
        }
        DesktopAuthority::Shortcut => ["profile", "bind", "pointer-bind"].contains(&name),
        DesktopAuthority::Session => [
            "application",
            "terminal",
            "browser",
            "logout",
            "startup",
            "control",
            "window-manager",
            "shell-client",
            "shell-config",
            "application-catalog",
        ]
        .contains(&name),
        DesktopAuthority::Input => {
            ["inherit-sophia", "keyboard", "pointer", "cursor"].contains(&name)
        }
        DesktopAuthority::Output => ["inherit-sophia", "named"].contains(&name),
        DesktopAuthority::Broker => ["enabled", "capability"].contains(&name),
    };
    if !supported {
        return Err(DesktopProfileError::Schema(format!(
            "unsupported {} setting {name:?}",
            authority.name()
        )));
    }
    if authority == DesktopAuthority::Shell
        && ["enabled", "content"].contains(&name)
        && (node.entries().len() != 1
            || node.children().is_some()
            || node.get(0).and_then(|value| value.as_bool()).is_none())
    {
        return Err(DesktopProfileError::Schema(format!(
            "shell {name} requires one boolean argument"
        )));
    }
    // `shell.panel` is the bottom-edge work-area strip, in pixels. It was an
    // allowlisted name with no validation and no reader, so a profile could
    // ask for a panel and be silently ignored; it now means exactly one thing
    // and is refused where it cannot be honoured. The ceiling is the wire's,
    // so a profile cannot promise a claim `sophia_shell_v1` would reject.
    if authority == DesktopAuthority::Shell && name == "panel" {
        let value = exact_integer_argument(node, "shell panel")?;
        if !(0..=i128::from(SHELL_PANEL_MAX_THICKNESS_PX)).contains(&value) {
            return Err(DesktopProfileError::Schema(
                "shell panel must be a pixel thickness within the reservation maximum".to_owned(),
            ));
        }
    }
    if authority == DesktopAuthority::Shell && name == "gpu-memory-bytes" {
        let value = exact_integer_argument(node, "shell gpu-memory-bytes")?;
        if value != i128::from(SHELL_GPU_MEMORY_BYTES) {
            return Err(DesktopProfileError::Schema(format!(
                "shell gpu-memory-bytes must be the admitted prototype limit {}",
                SHELL_GPU_MEMORY_BYTES
            )));
        }
    }
    if authority == DesktopAuthority::Broker
        && name == "enabled"
        && node.get(0).and_then(|value| value.as_bool()) != Some(false)
    {
        return Err(DesktopProfileError::Schema(
            "unavailable capability cannot be enabled".to_owned(),
        ));
    }
    Ok(())
}

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

/// Returns the exact configured GPU limit. It has no implicit default.
pub fn desktop_profile_shell_gpu_memory_bytes(profile: &DesktopProfileGeneration) -> Option<u64> {
    profile
        .candidates
        .get(&DesktopAuthority::Shell)
        .and_then(|candidate| {
            candidate
                .values
                .iter()
                .find(|value| value.key == "shell.gpu-memory-bytes")
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
        .and_then(|bytes| u64::try_from(bytes).ok())
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

fn setting_key(authority: DesktopAuthority, node: &KdlNode) -> Result<String, DesktopProfileError> {
    let mut key = format!("{}.{}", authority.name(), node.name().value());
    if authority != DesktopAuthority::Policy
        && ["bind", "pointer-bind", "application", "device", "named"].contains(&node.name().value())
    {
        key.push('.');
        key.push_str(exact_first_string(node)?);
    }
    Ok(key)
}

fn exact_string_argument<'a>(
    node: &'a KdlNode,
    context: &str,
) -> Result<&'a str, DesktopProfileError> {
    if node.entries().len() != 1 || node.children().is_some() || node.ty().is_some() {
        return Err(DesktopProfileError::Schema(format!(
            "{context} requires one string"
        )));
    }
    exact_first_string(node)
}

fn exact_first_string(node: &KdlNode) -> Result<&str, DesktopProfileError> {
    node.get(0)
        .and_then(|value| value.as_string())
        .ok_or_else(|| DesktopProfileError::Schema("string identity required".to_owned()))
}

fn exact_integer_argument(node: &KdlNode, context: &str) -> Result<i128, DesktopProfileError> {
    if node.entries().len() != 1 || node.children().is_some() || node.ty().is_some() {
        return Err(DesktopProfileError::Schema(format!(
            "{context} requires one integer"
        )));
    }
    node.get(0)
        .and_then(|value| value.as_integer())
        .ok_or_else(|| DesktopProfileError::Schema(format!("{context} requires one integer")))
}

fn integer_argument(node: &KdlNode) -> Option<i128> {
    (node.entries().len() == 1 && node.ty().is_none())
        .then(|| node.get(0))
        .flatten()
        .and_then(|value| value.as_integer())
}
