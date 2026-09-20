// The render device the external X probes hand the authority.
//
// Extracted from the smokes it serves because three of them build one and the
// file that held it had grown past the reviewed size. This file is included,
// not a module, so its heading is an ordinary comment.

struct ExternalProbeRenderDeviceProvider {
    device: std::fs::File,
    import_formats: Vec<sophia_x_authority::XServerFrontendDmaBufImportFormat>,
}

impl ExternalProbeRenderDeviceProvider {
    /// Measures the node's DMA-BUF import inventory once, before the frontend
    /// exists, the way a live session does.
    ///
    /// The authority answers `GetSupportedModifiers` from this inventory and
    /// nothing else. Handed none, mesa allocates without a modifier and
    /// imports through the single-plane `PixmapFromBuffer`, which is not the
    /// path a session runs and not the stage the GLX probes require. A node
    /// that cannot be measured is reported and advertised as nothing, as the
    /// session does, so a probe that needs the layouts says which stage it
    /// lost rather than failing to start.
    fn measured(device: std::fs::File) -> Result<Self, Box<dyn std::error::Error>> {
        let import_formats = match measure_dma_buf_import_formats(&device) {
            Ok(formats) => formats,
            Err(reason) => {
                eprintln!(
                    "sophia_x_authority_probe schema=1 status=degraded reason=dma_buf_import_capabilities_unavailable error={reason}"
                );
                Vec::new()
            }
        };
        Ok(Self {
            device,
            import_formats,
        })
    }
}

/// The node's DMA-BUF import inventory, measured by the backend the live
/// session uses.
#[cfg(feature = "native-session")]
fn measure_dma_buf_import_formats(
    device: &std::fs::File,
) -> Result<Vec<sophia_x_authority::XServerFrontendDmaBufImportFormat>, String> {
    device
        .try_clone()
        .map_err(|_| sophia_backend_live::LiveDmaBufCapabilityError::DeviceUnavailable)
        .and_then(sophia_backend_live::query_dma_buf_import_formats)
        .map(|formats| {
            formats
                .into_iter()
                .map(
                    |row| sophia_x_authority::XServerFrontendDmaBufImportFormat {
                        format: row.format,
                        modifiers: row.modifiers,
                    },
                )
                .collect()
        })
        .map_err(|error| format!("{error:?}"))
}

/// Without the native session, the backend that measures a node is not
/// built. The inventory is then empty and the probe says so: the same
/// degraded path a node that cannot be measured takes, so the probes still
/// build and run in the default configuration.
#[cfg(not(feature = "native-session"))]
fn measure_dma_buf_import_formats(
    _device: &std::fs::File,
) -> Result<Vec<sophia_x_authority::XServerFrontendDmaBufImportFormat>, String> {
    Err("native session feature not built".into())
}

impl XServerFrontendRenderDeviceProvider for ExternalProbeRenderDeviceProvider {
    fn dma_buf_import_formats(&self) -> Vec<sophia_x_authority::XServerFrontendDmaBufImportFormat> {
        self.import_formats.clone()
    }

    fn open_render_device_fd(
        &self,
    ) -> Result<std::os::fd::OwnedFd, XServerFrontendRenderDeviceError> {
        use std::os::fd::AsRawFd as _;

        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!("/proc/self/fd/{}", self.device.as_raw_fd()))
            .map(std::os::fd::OwnedFd::from)
            .map_err(|_| XServerFrontendRenderDeviceError::OpenFailed)
    }
}
