use super::*;

mod render_device;
pub(super) mod xtest;

pub(super) struct LiveXAdmissionPolicy {
    pub(super) launch_origins: Arc<Mutex<crate::launch_origin::LaunchOriginRegistry>>,
    pub(super) registry: Arc<Mutex<NamespaceRegistry>>,
    pub(super) namespace: NamespaceId,
    pub(super) session_user_id: u32,
}

impl XServerFrontendAdmissionPolicy for LiveXAdmissionPolicy {
    fn admit(
        &self,
        request: XServerFrontendAdmissionRequest,
    ) -> Result<ClientAdmissionContext, XServerFrontendAdmissionError> {
        let peer = request
            .peer_credentials
            .ok_or(XServerFrontendAdmissionError::Denied)?;
        if peer.user_id != self.session_user_id {
            return Err(XServerFrontendAdmissionError::Denied);
        }
        let context = self
            .registry
            .lock()
            .map_err(|_| XServerFrontendAdmissionError::Unavailable)?
            .admit(self.namespace, request.setup_authentication)
            .map_err(|_| XServerFrontendAdmissionError::Unavailable)?;
        if let Some(start_time) = peer.process_start_time {
            let identity = crate::launch_origin::ProcessIdentity {
                pid: peer.process_id,
                start_time,
            };
            // Proc I/O runs on this admission worker, never on the display loop.
            let ancestors = crate::launch_origin::process_ancestors(
                identity,
                crate::launch_origin::read_process,
            );
            if let Ok(mut origins) = self.launch_origins.lock() {
                origins.admit(context, identity, &ancestors);
            }
        }
        Ok(context)
    }

    fn revoke(&self, context: ClientAdmissionContext) -> Result<(), XServerFrontendAdmissionError> {
        if context.namespace.id != self.namespace {
            return Err(XServerFrontendAdmissionError::Unavailable);
        }
        if let Ok(mut origins) = self.launch_origins.lock() {
            origins.revoke(context);
        }
        self.registry
            .lock()
            .map_err(|_| XServerFrontendAdmissionError::Unavailable)?
            .revoke_admission(context.client_id)
            .map(|_| ())
            .map_err(|_| XServerFrontendAdmissionError::Unavailable)
    }
}

/// Originates buffers for pixmaps a client did not allocate.
///
/// Holds the same device the render node is handed from, so a buffer the
/// authority originates and one the client imports come from one device and can
/// be composited by the same path.
#[cfg(feature = "native-session")]
pub(super) struct LiveXPixmapAllocator {
    pub(super) device: std::fs::File,
    shared: Option<sophia_backend_live::LiveSharedPixmapService>,
}

#[cfg(feature = "native-session")]
impl LiveXPixmapAllocator {
    pub(super) fn without_pixmap_textures(device: std::fs::File) -> Self {
        Self {
            device,
            shared: None,
        }
    }

    pub(super) fn new(device: std::fs::File) -> Self {
        let shared = device
            .try_clone()
            .map_err(|_| sophia_backend_live::LiveSharedPixmapError::Unavailable)
            .and_then(sophia_backend_live::LiveSharedPixmapService::new);
        let shared = match shared {
            Ok(service) => Some(service),
            Err(reason) => {
                tracing::warn!(?reason, "pixmap texture export capability unavailable");
                None
            }
        };
        Self { device, shared }
    }
}

#[cfg(feature = "native-session")]
impl XServerFrontendPixmapAllocator for LiveXPixmapAllocator {
    fn allocate_pixmap_buffer(
        &self,
        request: XServerFrontendPixmapAllocation,
    ) -> Result<XServerFrontendAllocatedPixmap, XServerFrontendPixmapAllocationError> {
        if let Some(shared) = self.shared.as_ref() {
            let allocation = shared
                .allocate(
                    sophia_protocol::BufferHandle::from_raw(request.handle),
                    request.size,
                    request.depth,
                )
                .map_err(shared_pixmap_error)?;
            return Ok(XServerFrontendAllocatedPixmap {
                descriptor: allocation.descriptor,
                plane_fds: allocation.plane_fds,
            });
        }
        let allocation = sophia_backend_live::allocate_shared_buffer(
            &self.device,
            request.handle,
            request.size,
            request.depth,
        )
        .map_err(|error| match error {
            sophia_backend_live::LiveSharedBufferError::UnsupportedTarget => {
                XServerFrontendPixmapAllocationError::UnsupportedTarget
            }
            sophia_backend_live::LiveSharedBufferError::DeviceRejected
            | sophia_backend_live::LiveSharedBufferError::ExportFailed => {
                XServerFrontendPixmapAllocationError::AllocationFailed
            }
        })?;
        Ok(XServerFrontendAllocatedPixmap {
            descriptor: allocation.descriptor,
            plane_fds: allocation.plane_fds,
        })
    }

    fn supports_pixmap_textures(&self) -> bool {
        self.shared.is_some()
    }

    fn update_pixmap_buffer(
        &self,
        request: sophia_x_authority::XServerFrontendPixmapUpdate,
    ) -> Result<(), XServerFrontendPixmapAllocationError> {
        let shared = self
            .shared
            .as_ref()
            .ok_or(XServerFrontendPixmapAllocationError::Unavailable)?;
        shared
            .update(sophia_backend_live::LiveSharedPixmapUpdate {
                handle: request.handle,
                revision: request.revision,
                size: request.size,
                format: request.format,
                patches: request
                    .patches
                    .into_iter()
                    .map(|patch| sophia_backend_live::LiveSharedPixmapPatch {
                        rect: patch.rect,
                        bytes: patch.bytes,
                    })
                    .collect(),
            })
            .map_err(shared_pixmap_error)
    }

    fn release_pixmap_buffer(
        &self,
        handle: sophia_protocol::BufferHandle,
    ) -> Result<(), XServerFrontendPixmapAllocationError> {
        self.shared
            .as_ref()
            .ok_or(XServerFrontendPixmapAllocationError::Unavailable)?
            .release(handle)
            .map_err(shared_pixmap_error)
    }
}

#[cfg(feature = "native-session")]
fn shared_pixmap_error(
    error: sophia_backend_live::LiveSharedPixmapError,
) -> XServerFrontendPixmapAllocationError {
    use sophia_backend_live::LiveSharedPixmapError as E;
    match error {
        E::InvalidTarget => XServerFrontendPixmapAllocationError::UnsupportedTarget,
        E::UnknownBacking => XServerFrontendPixmapAllocationError::UnknownBacking,
        E::Unavailable => XServerFrontendPixmapAllocationError::Unavailable,
        E::Capacity | E::IdentityInUse | E::DeviceRejected | E::ExportFailed | E::UploadFailed => {
            XServerFrontendPixmapAllocationError::AllocationFailed
        }
    }
}

pub(super) struct LiveXRenderDeviceProvider {
    pub(super) device: std::fs::File,
    pub(super) import_formats: Vec<sophia_x_authority::XServerFrontendDmaBufImportFormat>,
}

impl XServerFrontendRenderDeviceProvider for LiveXRenderDeviceProvider {
    fn render_device_identity(&self) -> Option<sophia_x_authority::XRenderDeviceIdentity> {
        let stat = rustix::fs::fstat(&self.device).ok()?;
        Some(sophia_x_authority::XRenderDeviceIdentity {
            device: stat.st_dev,
            inode: stat.st_ino,
            device_number: stat.st_rdev,
        })
    }

    fn dma_buf_import_formats(&self) -> Vec<sophia_x_authority::XServerFrontendDmaBufImportFormat> {
        self.import_formats.clone()
    }

    fn open_render_device_fd(
        &self,
    ) -> Result<std::os::fd::OwnedFd, XServerFrontendRenderDeviceError> {
        render_device::open_render_device(&self.device).map_err(|reason| {
            tracing::warn!(%reason, "DRI3 render device open refused");
            if reason.kind() == std::io::ErrorKind::Other {
                XServerFrontendRenderDeviceError::OpenFailed
            } else {
                XServerFrontendRenderDeviceError::Unavailable
            }
        })
    }
}
