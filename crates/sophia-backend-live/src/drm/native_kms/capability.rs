use crate::prelude::*;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LibdrmNativeOutputTiming {
    pub width: u32,
    pub height: u32,
    /// The nominal rate: DRM's integer `vrefresh` scaled by a thousand. This
    /// is what a profile writes as `@120` and what the mode matcher compares.
    pub refresh_millihz: u32,
    /// The scanout timing the mode actually carries.
    ///
    /// `None` where the timing was never read from a mode -- a synthetic
    /// output in a test, say. Keeping the distinction means a consumer can
    /// tell "not measured" from "measured as zero".
    pub mode: Option<sophia_protocol::OutputModeTiming>,
}

impl LibdrmNativeOutputTiming {
    /// A timing known only by its extent and nominal rate.
    ///
    /// Retained for synthetic outputs, which have no mode to read. The real
    /// path is `from_mode`, which keeps what DRM reported.
    pub const fn new(width: u32, height: u32, refresh_millihz: u32) -> Self {
        Self {
            width,
            height,
            refresh_millihz,
            mode: None,
        }
    }

    pub const fn valid(self) -> bool {
        self.width > 0 && self.height > 0 && self.refresh_millihz > 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibdrmNativeModeResolutionStatus {
    Resolved,
    /// The requested timing is not one this connector advertises. A planned
    /// candidate that cannot name a real mode must not reach a commit.
    UnknownTiming,
    /// The request itself carried a zero dimension or refresh.
    InvalidTiming,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibdrmNativeModeResolution {
    pub status: LibdrmNativeModeResolutionStatus,
    /// Index into the connector's mode list, in the order the kernel reported it.
    pub index: Option<usize>,
}

impl LibdrmNativeModeResolution {
    const fn rejected(status: LibdrmNativeModeResolutionStatus) -> Self {
        Self {
            status,
            index: None,
        }
    }
}

/// Resolves a planned timing to a position in a connector's reported mode list.
///
/// A configured candidate names a timing, but a KMS commit needs the mode object
/// that produced it, and the reduction from mode to timing is lossy: several modes
/// can share one width, height, and integer refresh. This returns the **first**
/// nominal match when the request omits a modeline. A request carrying a full
/// modeline must match it exactly, preserving an opaque authority mode selection.
///
/// Invalid modes are skipped rather than matched, for the same reason the
/// capability reader skips them: a zero dimension or refresh cannot drive a head.
pub fn resolve_native_output_mode_index(
    modes: &[LibdrmNativeOutputTiming],
    requested: LibdrmNativeOutputTiming,
) -> LibdrmNativeModeResolution {
    if !requested.valid() {
        return LibdrmNativeModeResolution::rejected(
            LibdrmNativeModeResolutionStatus::InvalidTiming,
        );
    }
    match modes.iter().position(|mode| {
        mode.valid()
            && mode.width == requested.width
            && mode.height == requested.height
            && mode.refresh_millihz == requested.refresh_millihz
            && requested
                .mode
                .is_none_or(|timing| mode.mode == Some(timing))
    }) {
        Some(index) => LibdrmNativeModeResolution {
            status: LibdrmNativeModeResolutionStatus::Resolved,
            index: Some(index),
        },
        None => {
            LibdrmNativeModeResolution::rejected(LibdrmNativeModeResolutionStatus::UnknownTiming)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibdrmNativeOutputCapability {
    head: sophia_engine::RenderHeadId,
    output: OutputId,
    connector_id: u32,
    connector_name: String,
    gpu_identity: Option<String>,
    connector_key: String,
    modes: Vec<LibdrmNativeOutputTiming>,
    preferred_mode: Option<LibdrmNativeOutputTiming>,
    selected_mode: LibdrmNativeOutputTiming,
    vrr_status: LibdrmNativeVrrPropertyDiscoveryStatus,
}

impl LibdrmNativeOutputCapability {
    pub fn new(
        output: OutputId,
        connector_id: u32,
        connector_name: impl Into<String>,
        modes: impl IntoIterator<Item = LibdrmNativeOutputTiming>,
        preferred_mode: Option<LibdrmNativeOutputTiming>,
        selected_mode: LibdrmNativeOutputTiming,
        vrr_status: LibdrmNativeVrrPropertyDiscoveryStatus,
    ) -> io::Result<Self> {
        let connector_name = connector_name.into();
        let capability = Self {
            head: sophia_engine::RenderHeadId::INVALID,
            output,
            connector_id,
            connector_key: connector_name.clone(),
            connector_name,
            gpu_identity: None,
            modes: modes.into_iter().collect(),
            preferred_mode,
            selected_mode,
            vrr_status,
        };
        if capability.connector_name.is_empty()
            || capability.connector_name.len() > 64
            || !capability
                .connector_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(io::Error::other("DRM connector identity is invalid"));
        }
        if capability.modes.is_empty()
            || capability.modes.len() > 256
            || capability.modes.iter().any(|mode| !mode.valid())
            || capability
                .preferred_mode
                .is_some_and(|mode| !capability.modes.contains(&mode))
            || !capability.modes.contains(&capability.selected_mode)
        {
            return Err(io::Error::other(
                "DRM connector has inconsistent mode capabilities",
            ));
        }
        Ok(capability)
    }

    pub const fn output(&self) -> OutputId {
        self.output
    }

    /// Opaque Engine/backend identity when this capability came from an active
    /// production head. Standalone discovery and test fixtures may not yet have
    /// admitted a head, in which case this is `None`.
    pub const fn head(&self) -> Option<sophia_engine::RenderHeadId> {
        if self.head.is_valid() {
            Some(self.head)
        } else {
            None
        }
    }

    pub fn bind_head(mut self, head: sophia_engine::RenderHeadId) -> io::Result<Self> {
        if !head.is_valid() {
            return Err(io::Error::other(
                "DRM capability cannot bind an invalid head",
            ));
        }
        self.head = head;
        Ok(self)
    }

    pub const fn connector_id(&self) -> u32 {
        self.connector_id
    }

    pub fn connector_name(&self) -> &str {
        &self.connector_name
    }

    pub fn gpu_identity(&self) -> Option<&str> {
        self.gpu_identity.as_deref()
    }

    pub fn connector_key(&self) -> &str {
        &self.connector_key
    }

    pub fn with_gpu_identity(mut self, gpu: Option<&str>) -> io::Result<Self> {
        if gpu.is_some_and(|gpu| !crate::gpu_admission::valid_identity(gpu)) {
            return Err(io::Error::other("DRM GPU identity is invalid"));
        }
        self.gpu_identity = gpu.map(str::to_owned);
        self.connector_key = gpu.map_or_else(
            || self.connector_name.clone(),
            |gpu| format!("{gpu}/{}", self.connector_name),
        );
        Ok(self)
    }

    pub fn modes(&self) -> &[LibdrmNativeOutputTiming] {
        &self.modes
    }

    pub const fn preferred_mode(&self) -> Option<LibdrmNativeOutputTiming> {
        self.preferred_mode
    }

    pub const fn selected_mode(&self) -> LibdrmNativeOutputTiming {
        self.selected_mode
    }

    pub const fn vrr_status(&self) -> LibdrmNativeVrrPropertyDiscoveryStatus {
        self.vrr_status
    }

    pub const fn vrr_configurable(&self) -> bool {
        matches!(
            self.vrr_status,
            LibdrmNativeVrrPropertyDiscoveryStatus::Discovered
        )
    }
}

pub(crate) fn read_native_output_capability<D>(
    device: &D,
    selection: LibdrmNativePrimaryPlaneSelection,
    output: OutputId,
) -> io::Result<LibdrmNativeOutputCapability>
where
    D: drm::control::Device + LibdrmNativePropertyLookupDevice,
{
    let connector = device.get_connector(selection.connector, false)?;
    let connector_name = connector.to_string();
    let mut modes = Vec::new();
    let mut preferred_mode = None;
    for mode in connector.modes().iter().copied() {
        let timing = native_output_timing(mode);
        if !timing.valid() {
            continue;
        }
        if preferred_mode.is_none()
            && mode
                .mode_type()
                .contains(drm::control::ModeTypeFlags::PREFERRED)
        {
            preferred_mode = Some(timing);
        }
        if !modes.contains(&timing) {
            modes.push(timing);
        }
    }
    let selected_mode = selection
        .mode
        .map(native_output_timing)
        .filter(|mode| mode.valid())
        .ok_or_else(|| io::Error::other("selected DRM connector has no usable mode"))?;
    let vrr_status =
        discover_native_vrr_properties(device, selection.connector, selection.crtc).status;
    LibdrmNativeOutputCapability::new(
        output,
        selection.connector_id(),
        connector_name,
        modes,
        preferred_mode,
        selected_mode,
        vrr_status,
    )
}

/// Reads a connector's modes and returns the one matching a planned timing.
///
/// This is the bridge from a configured candidate to a KMS mode object. It reduces
/// each reported mode exactly as `read_native_output_capability` does and defers
/// the choice to `resolve_native_output_mode_index`, so capability advertisement
/// and commit selection cannot disagree. Returning `None` is a fail-closed
/// outcome, not an error: the connector simply does not offer that timing.
#[cfg(feature = "libdrm-events")]
pub fn resolve_native_connector_mode<D>(
    device: &D,
    connector: drm::control::connector::Handle,
    requested: LibdrmNativeOutputTiming,
) -> io::Result<Option<drm::control::Mode>>
where
    D: drm::control::Device,
{
    let reported = device.get_connector(connector, false)?;
    let modes = reported.modes();
    let reduced = modes
        .iter()
        .copied()
        .map(native_output_timing)
        .collect::<Vec<_>>();
    let resolution = resolve_native_output_mode_index(&reduced, requested);
    Ok(resolution.index.map(|index| modes[index]))
}

/// Keeps what DRM said about a mode, rather than a summary of it.
///
/// This used to discard everything but the extent and a rounded refresh, which
/// left the server unable to answer any client that asked how the display is
/// actually scanned -- and left RandR inventing a modeline with no blanking at
/// all. The mode is right here; there was never a reason not to keep it.
pub(crate) fn native_output_timing(mode: drm::control::Mode) -> LibdrmNativeOutputTiming {
    let (width, height) = mode.size();
    let (hsync_start, hsync_end, htotal) = mode.hsync();
    let (vsync_start, vsync_end, vtotal) = mode.vsync();
    LibdrmNativeOutputTiming {
        width: u32::from(width),
        height: u32::from(height),
        refresh_millihz: mode.vrefresh().saturating_mul(1_000),
        mode: Some(sophia_protocol::OutputModeTiming {
            clock_khz: mode.clock(),
            hdisplay: width,
            hsync_start,
            hsync_end,
            htotal,
            hskew: mode.hskew(),
            vdisplay: height,
            vsync_start,
            vsync_end,
            vtotal,
            flags: mode.flags().bits(),
        }),
    }
}
