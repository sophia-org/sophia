//! Opt-in environment facts per head for timing evidence, read from the
//! already owned DRM cards and renderer state. Nothing is opened, probed or
//! submitted. A fact that cannot be read stays `None`; nothing is inferred
//! from configuration or policy. The KMS record is the unchanged rollback
//! readback, so its equality checks are unaffected.
use super::{LiveProductionNativeScanout, LiveProductionOutputKmsReadback};
use drm::Device as _;
use drm::control::Device as _;
use std::os::fd::{AsFd, AsRawFd};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveProductionOutputEnvironmentReadback {
    pub kms: LiveProductionOutputKmsReadback,
    pub output: sophia_protocol::OutputId,
    /// The device node name of the owned card, such as `card1`.
    pub card_node: Option<String>,
    /// The kernel's connector name, such as `DP-2`.
    pub connector_name: Option<String>,
    /// The card's kernel driver name.
    pub driver: Option<String>,
    /// The read-back mode's whole-Hz `vrefresh` times 1000. This is the
    /// rounded nominal rate Sophia's capability table uses, not a precise rate.
    pub nominal_refresh_millihz: Option<u32>,
    /// The rate derived from the read-back programmed timing. It is not a
    /// measured scanout rate.
    pub timing_refresh_millihz: Option<u32>,
    /// The compositor-applied integer scale times 1000. KMS has no scale.
    pub applied_scale_milli: Option<u32>,
    /// The committed CRTC `VRR_ENABLED` value; `None` when the CRTC has no
    /// such property. It does not show whether the sink runs adaptive sync.
    pub vrr_enabled: Option<bool>,
    pub gl_identity: sophia_renderer_live::NativeGlIdentityState,
}

// Indices into `LiveProductionOutputKmsReadback::mode`.
const VREFRESH: usize = 2;
const CLOCK_KHZ: usize = 3;
const HTOTAL: usize = 6;
const VTOTAL: usize = 9;
const VSCAN: usize = 11;
const FLAGS: usize = 12;

impl LiveProductionNativeScanout {
    /// Environment facts for every head, each beside its unchanged KMS record.
    pub fn output_environment_readback(
        &self,
    ) -> std::io::Result<Vec<LiveProductionOutputEnvironmentReadback>> {
        let mut records = Vec::with_capacity(self.heads.len());
        for kms in self.output_topology_kms_readback()? {
            let index = self
                .heads
                .iter()
                .position(|head| head.head == kms.head)
                .ok_or_else(|| std::io::Error::other("readback head is no longer live"))?;
            let head = &self.heads[index];
            let card = self.groups[head.group].session.card();
            let connector = card
                .get_connector(head.selection.connector_handle(), false)
                .ok()
                .and_then(|info| {
                    LiveProductionOutputEnvironmentReadback::connector_name(
                        info.interface().as_str(),
                        info.interface_id(),
                    )
                });
            let driver = card
                .get_driver()
                .ok()
                .and_then(|driver| driver.name().to_str().map(str::to_owned))
                .filter(|name| !name.is_empty());
            let vrr_enabled = LiveProductionOutputEnvironmentReadback::vrr_enabled(
                kms.properties.get("crtc.VRR_ENABLED").copied(),
            )?;
            records.push(LiveProductionOutputEnvironmentReadback {
                output: head.output.id,
                card_node: owned_card_node(card),
                connector_name: connector,
                driver,
                nominal_refresh_millihz: kms
                    .mode
                    .as_ref()
                    .and_then(LiveProductionOutputEnvironmentReadback::nominal_refresh_millihz),
                timing_refresh_millihz: kms
                    .mode
                    .as_ref()
                    .and_then(LiveProductionOutputEnvironmentReadback::timing_refresh_millihz),
                applied_scale_milli: LiveProductionOutputEnvironmentReadback::applied_scale_milli(
                    head.scale,
                ),
                vrr_enabled,
                gl_identity: self
                    .exporters
                    .get(index)
                    .map(|exporter| exporter.gl_identity())
                    .unwrap_or_default(),
                kms,
            });
        }
        Ok(records)
    }
}

/// The node name of an owned card descriptor, accepted only when the path it
/// was opened from still names the same character device.
fn owned_card_node(card: &impl AsFd) -> Option<String> {
    let held = rustix::fs::fstat(card).ok()?;
    let link = std::fs::read_link(format!("/proc/self/fd/{}", card.as_fd().as_raw_fd())).ok()?;
    let path_rdev = rustix::fs::stat(&link).ok().map(|stat| stat.st_rdev);
    LiveProductionOutputEnvironmentReadback::card_node_name(&link, held.st_rdev, path_rdev)
}

/// Pure conversions behind the readback, public so they are tested without a
/// device.
impl LiveProductionOutputEnvironmentReadback {
    pub const MODE_VREFRESH: usize = VREFRESH;
    pub const MODE_CLOCK_KHZ: usize = CLOCK_KHZ;
    pub const MODE_HTOTAL: usize = HTOTAL;
    pub const MODE_VTOTAL: usize = VTOTAL;
    pub const MODE_VSCAN: usize = VSCAN;
    pub const MODE_FLAGS: usize = FLAGS;

    pub fn card_node_name(
        link: &std::path::Path,
        held_rdev: u64,
        path_rdev: Option<u64>,
    ) -> Option<String> {
        let name = link.file_name()?.to_str()?;
        let digits = name.strip_prefix("card")?;
        (path_rdev == Some(held_rdev)
            && !digits.is_empty()
            && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| name.to_owned())
    }

    /// The kernel's `<interface>-<id>` connector name. Unknown interfaces have
    /// no name.
    pub fn connector_name(interface: &str, interface_id: u32) -> Option<String> {
        (interface != "Unknown" && !interface.is_empty())
            .then(|| format!("{interface}-{interface_id}"))
    }

    pub fn nominal_refresh_millihz(mode: &[u32; 13]) -> Option<u32> {
        mode[VREFRESH].checked_mul(1_000).filter(|&rate| rate != 0)
    }

    /// `clock * 1e6 / (htotal * vtotal)` in mHz, rounded to nearest, with the
    /// kernel's `drm_mode_vrefresh` factors: interlace doubles the field rate,
    /// doublescan halves it and a `vscan` above one divides it.
    pub fn timing_refresh_millihz(mode: &[u32; 13]) -> Option<u32> {
        let flags = drm::control::ModeFlags::from_bits_truncate(mode[FLAGS]);
        let mut numerator = u128::from(mode[CLOCK_KHZ]) * 1_000_000;
        let mut denominator = u128::from(mode[HTOTAL]) * u128::from(mode[VTOTAL]);
        if flags.contains(drm::control::ModeFlags::INTERLACE) {
            numerator *= 2;
        }
        if flags.contains(drm::control::ModeFlags::DBLSCAN) {
            denominator *= 2;
        }
        if mode[VSCAN] > 1 {
            denominator *= u128::from(mode[VSCAN]);
        }
        if denominator == 0 {
            return None;
        }
        let rate = (numerator + denominator / 2) / denominator;
        u32::try_from(rate).ok().filter(|&rate| rate != 0)
    }

    pub fn applied_scale_milli(scale: u32) -> Option<u32> {
        scale.checked_mul(1_000).filter(|&scale| scale != 0)
    }

    /// `VRR_ENABLED` is a boolean property; any other value is a read error.
    pub fn vrr_enabled(value: Option<u64>) -> std::io::Result<Option<bool>> {
        match value {
            None => Ok(None),
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            Some(other) => Err(std::io::Error::other(format!(
                "crtc VRR_ENABLED read back {other}, not a boolean"
            ))),
        }
    }
}
