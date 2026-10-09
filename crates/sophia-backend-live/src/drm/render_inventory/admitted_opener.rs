use super::*;

/// A probe's admitted identity, retained across independent renderer instances.
/// Cloning this value opens nothing and shares no DRM file description.
#[derive(Clone, Debug)]
pub struct LiveAdmittedRenderDeviceOpener {
    primary: SeatDrmCard,
    render: RenderCandidate,
    identity: LiveRenderDeviceIdentitySnapshot,
}

impl LiveAdmittedRenderDeviceOpener {
    #[cfg(feature = "seat-control")]
    pub(crate) fn from_primary(
        primary: &SeatDrmCard,
    ) -> Result<Self, LiveRenderDeviceInventoryError> {
        use LiveRenderDeviceInventoryError as E;
        primary
            .validate_current(&primary.seat)
            .map_err(|_| E::IdentityChanged)?;
        let render =
            selection::render_sibling(Path::new("/sys/class/drm"), &primary.physical_device)?
                .ok_or(E::DiscoveryUnavailable)?;
        let identity = snapshot_candidate(&render)?;
        primary
            .validate_current(&primary.seat)
            .map_err(|_| E::IdentityChanged)?;
        Ok(Self {
            primary: primary.clone(),
            render,
            identity,
        })
    }

    pub fn primary_node(&self) -> &Path {
        &self.primary.node
    }

    pub fn primary_device_number(&self) -> u64 {
        self.primary.device_number
    }

    pub fn gpu_identity(&self) -> Option<&std::ffi::OsStr> {
        self.primary.gpu_id.as_deref()
    }

    pub fn render_identity(&self) -> &LiveRenderDeviceIdentitySnapshot {
        &self.identity
    }

    /// Always a fresh render-node open. No primary-node or dup fallback exists.
    pub fn open(&self) -> Result<LiveRenderDevice, LiveRenderDeviceInventoryError> {
        open_revalidated(
            &self.identity,
            || {
                self.primary
                    .validate_current(&self.primary.seat)
                    .map_err(|_| LiveRenderDeviceInventoryError::IdentityChanged)
            },
            || snapshot_candidate(&self.render),
            || open_candidate(self.render.clone()),
        )
    }
}

fn open_revalidated(
    identity: &LiveRenderDeviceIdentitySnapshot,
    mut validate_primary: impl FnMut() -> Result<(), LiveRenderDeviceInventoryError>,
    snapshot: impl FnOnce() -> Result<LiveRenderDeviceIdentitySnapshot, LiveRenderDeviceInventoryError>,
    open: impl FnOnce() -> Result<LiveRenderDevice, LiveRenderDeviceInventoryError>,
) -> Result<LiveRenderDevice, LiveRenderDeviceInventoryError> {
    use LiveRenderDeviceInventoryError as E;
    validate_primary()?;
    if snapshot()? != *identity {
        return Err(E::IdentityChanged);
    }
    let opened = open()?;
    validate_primary()?;
    if opened.identity != *identity {
        return Err(E::IdentityChanged);
    }
    Ok(opened)
}

#[path = "../../../tests/support/admitted_render_opener.rs"]
mod tests;
