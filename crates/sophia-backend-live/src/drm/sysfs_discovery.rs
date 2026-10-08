use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sophia_engine::{
    DrmKmsMode, EngineHeadRegistry, HeadRenderTarget, OutputDiscoveryBackend, RenderHeadAllocator,
};
use sophia_protocol::OutputId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveDrmSysfsDiscoveryConfig {
    pub default_refresh_millihz: u32,
    pub default_scale: u32,
}

impl Default for LiveDrmSysfsDiscoveryConfig {
    fn default() -> Self {
        Self {
            default_refresh_millihz: 60_000,
            default_scale: 1,
        }
    }
}

/// Physical facts for one connected sysfs connector. This record stays inside
/// the backend: connector and CRTC integers never enter Engine records, which
/// receive only minted `RenderHeadId`s and reduced `HeadRenderTarget`s.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveSysfsConnectorRecord {
    pub connector_name: String,
    pub connector_id: u32,
    pub crtc_id: u32,
    pub mode: DrmKmsMode,
    pub scale: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveDrmSysfsDiscovery {
    config: LiveDrmSysfsDiscoveryConfig,
}

impl LiveDrmSysfsDiscovery {
    pub fn new(config: LiveDrmSysfsDiscoveryConfig) -> Self {
        Self { config }
    }

    /// Reads the connected connectors beneath a sysfs DRM root in stable
    /// (sorted) connector order.
    pub fn discover_connectors(
        &self,
        root: impl AsRef<Path>,
    ) -> io::Result<Vec<LiveSysfsConnectorRecord>> {
        self.discover_connectors_matching(root.as_ref(), |_| true)
    }

    /// Restricts discovery before reading connector facts. An unrelated seat's
    /// connector cannot affect completeness or make an admitted scan fail.
    #[cfg(all(
        feature = "seat-control",
        any(test, all(feature = "libdrm-events", feature = "gbm-probe"))
    ))]
    pub(crate) fn discover_connectors_on_cards(
        &self,
        root: &Path,
        cards: &[PathBuf],
    ) -> io::Result<Vec<LiveSysfsConnectorRecord>> {
        self.discover_connectors_matching(root, |path| {
            cards
                .iter()
                .any(|card| connector_belongs_to_card(path, card))
        })
    }

    fn discover_connectors_matching(
        &self,
        root: &Path,
        includes: impl Fn(&Path) -> bool,
    ) -> io::Result<Vec<LiveSysfsConnectorRecord>> {
        let mut records = Vec::new();
        for (index, path) in drm_connector_paths(root)?.into_iter().enumerate() {
            if !includes(&path) {
                continue;
            }
            let Some(record) = self.discover_connector(&path, index)? else {
                continue;
            };
            records.push(record);
        }
        Ok(records)
    }

    fn discover_connector(
        &self,
        path: &Path,
        index: usize,
    ) -> io::Result<Option<LiveSysfsConnectorRecord>> {
        if read_trimmed(path.join("status"))?.as_deref() != Some("connected") {
            return Ok(None);
        }

        let Some(mode) = read_first_mode(path, self.config.default_refresh_millihz)? else {
            return Ok(None);
        };
        let connector_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let connector_id = read_u32_file(path.join("connector_id"))?
            .unwrap_or_else(|| u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX));
        let crtc_id = read_u32_file(path.join("crtc_id"))?.unwrap_or(0);
        let scale = read_u32_file(path.join("scale"))?.unwrap_or(self.config.default_scale);

        Ok(Some(LiveSysfsConnectorRecord {
            connector_name,
            connector_id,
            crtc_id,
            mode,
            scale: scale.max(1),
        }))
    }
}

#[cfg(any(test, all(feature = "libdrm-events", feature = "gbm-probe")))]
pub(crate) fn connector_belongs_to_card(connector: &Path, card: &Path) -> bool {
    let Some(card) = card.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    connector
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(card))
        .is_some_and(|suffix| suffix.starts_with('-'))
}

#[cfg(any(test, all(feature = "libdrm-events", feature = "gbm-probe")))]
impl LiveSysfsConnectorRecord {
    /// DRM connector integers are local to a card, not global identifiers.
    pub(crate) fn matches_card_connector(&self, card: &Path, connector_id: u32) -> bool {
        self.connector_id == connector_id
            && connector_belongs_to_card(Path::new(&self.connector_name), card)
    }
}

pub fn discover_native_connector_records(
    root: impl AsRef<Path>,
) -> io::Result<Vec<LiveSysfsConnectorRecord>> {
    LiveDrmSysfsDiscovery::default().discover_connectors(root)
}

/// Sysfs-backed implementation of Engine's output discovery: one logical
/// output and one minted head per connected connector. Mirror grouping does
/// not exist at this layer; the native scanout path owns head-to-output
/// grouping for real KMS sessions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SysfsDrmKmsOutputBackend {
    root: PathBuf,
    discovery: LiveDrmSysfsDiscovery,
}

impl SysfsDrmKmsOutputBackend {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            discovery: LiveDrmSysfsDiscovery::default(),
        }
    }

    pub fn with_discovery(root: impl Into<PathBuf>, discovery: LiveDrmSysfsDiscovery) -> Self {
        Self {
            root: root.into(),
            discovery,
        }
    }
}

impl OutputDiscoveryBackend for SysfsDrmKmsOutputBackend {
    fn discover_outputs(&self) -> io::Result<EngineHeadRegistry> {
        let records = self.discovery.discover_connectors(&self.root)?;
        let mut allocator = RenderHeadAllocator::new();
        let mut registry = EngineHeadRegistry::new();
        for (index, record) in records.iter().enumerate() {
            let output =
                OutputId::from_raw(u64::try_from(index.saturating_add(1)).unwrap_or(u64::MAX));
            let target = HeadRenderTarget {
                head: allocator.mint(),
                output,
                target_generation: 1,
                native_size: record.mode.size,
                scale: record.scale,
                refresh_millihz: record.mode.refresh_millihz,
                transform: sophia_protocol::OutputTransform::Normal,
                mapping: sophia_protocol::OutputHeadMapping::Fit,
            };
            if !registry.admit(target).is_admitted() {
                return Err(io::Error::other(format!(
                    "sysfs DRM discovery exceeded Engine head capacity at connector index {index}"
                )));
            }
        }
        Ok(registry)
    }
}

fn drm_connector_paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("card") && name.contains('-') {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn read_first_mode(path: &Path, default_refresh_millihz: u32) -> io::Result<Option<DrmKmsMode>> {
    let Some(contents) = read_trimmed(path.join("modes"))? else {
        return Ok(None);
    };
    let Some(line) = contents.lines().find(|line| !line.trim().is_empty()) else {
        return Ok(None);
    };
    Ok(parse_drm_mode(line.trim(), default_refresh_millihz))
}

fn parse_drm_mode(mode: &str, refresh_millihz: u32) -> Option<DrmKmsMode> {
    let (width, height) = mode.split_once('x')?;
    Some(DrmKmsMode::new(
        width.parse().ok()?,
        height.parse().ok()?,
        refresh_millihz,
    ))
    .filter(|mode| mode.size.width > 0 && mode.size.height > 0)
}

fn read_u32_file(path: impl AsRef<Path>) -> io::Result<Option<u32>> {
    Ok(read_trimmed(path)?
        .as_deref()
        .and_then(|contents| contents.parse().ok()))
}

fn read_trimmed(path: impl AsRef<Path>) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents.trim().to_owned())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[path = "../../tests/support/seat_connector_discovery.rs"]
mod tests;
