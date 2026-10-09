use super::super::x_frontend::{LiveXPixmapAllocator, LiveXRenderDeviceProvider};
use super::*;
use sophia_x_authority::{XServerFrontendPixmapAllocator, XServerFrontendRenderDeviceProvider};

pub(super) fn prepare(
    request: PreparationRequest,
    admission: &sophia_backend_live::LiveGpuAdmission,
) -> Result<PreparedInventory, String> {
    let devices =
        sophia_backend_live::discover_admitted_seat_render_devices(&request.seat, admission)
            .map_err(|error| format!("render inventory: {error}"))?;
    let observed = devices
        .iter()
        .map(|device| device.identity.clone())
        .collect::<Vec<_>>();
    if observed != request.identities {
        return Err("render inventory changed during preparation".into());
    }
    let replacement = if request.replace_default {
        prepare_replacement(&request, &devices, |device| {
            device
                .file
                .try_clone()
                .map_err(|error| error.to_string())
                .and_then(|file| {
                    prepare_bundle(
                        request.bundle_generation,
                        file,
                        Some(request.pixmap_textures),
                    )
                })
        })
    } else {
        Ok(None)
    };
    Ok(PreparedInventory {
        devices,
        replacement,
    })
}

/// Each admitted device is tried at most once, with the previous physical device first.
pub(super) fn prepare_replacement(
    request: &PreparationRequest,
    devices: &[LiveRenderDevice],
    mut prepare: impl FnMut(&LiveRenderDevice) -> Result<Arc<Bundle>, String>,
) -> Result<Option<PreparedBundle>, String> {
    let preferred =
        |device: &&LiveRenderDevice| device.identity.physical_device == request.preferred_physical;
    let candidates = devices
        .iter()
        .filter(preferred)
        .chain(devices.iter().filter(|device| !preferred(device)));
    let mut reason = "seat has no render device for a replacement".to_owned();
    for device in candidates {
        match prepare(device) {
            Ok(bundle) => {
                return Ok(Some(PreparedBundle {
                    bundle,
                    identity: device.identity.clone(),
                }));
            }
            Err(error) => reason = error,
        }
    }
    Err(reason)
}

fn prepare_bundle(
    generation: u64,
    file: std::fs::File,
    contract: Option<bool>,
) -> Result<Arc<Bundle>, String> {
    let formats = match sophia_backend_live::query_dma_buf_import_formats(
        file.try_clone().map_err(|error| error.to_string())?,
    ) {
        Ok(formats) => formats,
        Err(error) if contract.is_none() => {
            tracing::warn!(
                generation,
                ?error,
                "initial render-device import query failed; explicit modifier inventory is empty"
            );
            Vec::new()
        }
        Err(error) => return Err(format!("render-device import capabilities: {error:?}")),
    };
    let allocator = if contract == Some(false) {
        LiveXPixmapAllocator::without_pixmap_textures(
            file.try_clone().map_err(|error| error.to_string())?,
        )
    } else {
        LiveXPixmapAllocator::new(file.try_clone().map_err(|error| error.to_string())?)
    };
    if contract.is_some_and(|expected| allocator.supports_pixmap_textures() != expected) {
        return Err("replacement cannot preserve the frontend pixmap-texture contract".into());
    }
    let provider = Arc::new(LiveXRenderDeviceProvider {
        device: file,
        import_formats: formats
            .into_iter()
            .map(
                |row| sophia_x_authority::XServerFrontendDmaBufImportFormat {
                    format: row.format,
                    modifiers: row.modifiers,
                },
            )
            .collect(),
    });
    Bundle::new(generation, provider, Some(Arc::new(allocator)))
        .map(Arc::new)
        .map_err(|error| error.to_string())
}

pub(in crate::live_session) fn initial(
    native: &sophia_backend_live::LiveProductionNativeScanout,
    seat: &str,
    admission: sophia_backend_live::LiveGpuAdmission,
) -> Result<(Arc<Bundle>, LiveRenderDeviceCoordinator), String> {
    use std::os::unix::fs::MetadataExt;
    let primary = LiveXRenderDeviceProvider {
        device: native
            .clone_render_device_file()
            .map_err(|error| error.to_string())?,
        import_formats: Vec::new(),
    };
    let file = std::fs::File::from(
        primary
            .open_render_device_fd()
            .map_err(|error| error.to_string())?,
    );
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let devices = sophia_backend_live::discover_admitted_seat_render_devices(seat, &admission)
        .map_err(|error| error.to_string())?;
    let identity = devices
        .iter()
        .find(|device| {
            device.identity.device == metadata.dev()
                && device.identity.inode == metadata.ino()
                && device.identity.device_number == metadata.rdev()
        })
        .ok_or_else(|| {
            "initial client render device is absent from the admitted seat inventory".to_owned()
        })?
        .identity
        .clone();
    let bundle = prepare_bundle(1, file, None)?;
    let coordinator = LiveRenderDeviceCoordinator::with_preparer(
        seat.to_owned(),
        bundle.clone(),
        identity,
        devices,
        move |request| prepare(request, &admission),
    )?;
    Ok((bundle, coordinator))
}
