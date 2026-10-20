use crate::LiveRendererImageId;

#[derive(Debug)]
pub struct LiveRendererImageSnapshot {
    pub(super) image_id: LiveRendererImageId,
    pub(super) inner: sophia_renderer_native_egl::NativeRendererImageSnapshot,
}

impl LiveRendererImageSnapshot {
    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Self {
            image_id: self.image_id,
            inner: self.inner.try_clone()?,
        })
    }

    pub const fn image_id(&self) -> LiveRendererImageId {
        self.image_id
    }

    /// What restoring it would charge a store, estimated from its size as
    /// four bytes a pixel; the store's own admission is authoritative.
    pub fn byte_estimate(&self) -> u64 {
        let frame = self.inner.as_frame();
        u64::from(frame.width)
            .saturating_mul(u64::from(frame.height))
            .saturating_mul(4)
    }
}

/// A fresh store's bounds: images it may hold, and their bytes.
pub const LIVE_RENDERER_IMAGE_STORE_CAPACITY: usize =
    sophia_renderer_native_egl::DEFAULT_NATIVE_RENDERER_IMAGE_CAPACITY;
pub const LIVE_RENDERER_IMAGE_STORE_BYTE_BUDGET: u64 =
    sophia_renderer_native_egl::DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET;

/// A budgeted immutable snapshot shared by queued frames, EGL imports and
/// submitted scanout owners. Cloning it neither copies pixels nor duplicates FDs.
#[derive(Clone, Debug)]
pub struct LiveRetainedRendererImageSnapshot {
    size: sophia_protocol::Size,
    pub(super) inner: std::sync::Arc<sophia_renderer_native_egl::NativeRendererImageSnapshot>,
}

/// Weak cleanup bookkeeping cannot keep an otherwise unused allocation alive.
#[derive(Clone, Debug)]
pub struct LiveRendererSnapshotWeak(
    std::sync::Weak<sophia_renderer_native_egl::NativeRendererImageSnapshot>,
);
impl LiveRendererSnapshotWeak {
    pub fn is_alive(&self) -> bool {
        self.0.strong_count() != 0
    }
}

impl LiveRetainedRendererImageSnapshot {
    /// Storage carried by the opaque completion, not a completion proof itself.
    #[cfg(feature = "experimental-shell-capture")]
    pub fn storage(&self) -> &std::sync::Arc<sophia_runtime::ContentCapturedImage> {
        &self.inner
    }

    /// Lower an already accepted lease without another registry, copy or charge.
    #[cfg(feature = "experimental-shell-capture")]
    pub(crate) fn from_content_lease(lease: &sophia_runtime::ContentResourceLease) -> Option<Self> {
        let inner = lease.captured_image()?;
        let frame = inner.as_frame();
        if !inner.is_retained() || !inner.is_current() || !frame.is_valid() {
            return None;
        }
        let size = sophia_protocol::Size {
            width: i32::try_from(frame.width).ok()?,
            height: i32::try_from(frame.height).ok()?,
        };
        Some(Self {
            size,
            inner: inner.clone(),
        })
    }

    /// The native adapter has already retained and charged this exact copy.
    #[cfg(feature = "experimental-shell-capture")]
    pub(crate) fn from_shell_capture(
        inner: std::sync::Arc<sophia_renderer_native_egl::NativeRendererImageSnapshot>,
    ) -> Self {
        let frame = inner.as_frame();
        let size = sophia_protocol::Size {
            width: i32::try_from(frame.width).expect("capture preflight bounded width"),
            height: i32::try_from(frame.height).expect("capture preflight bounded height"),
        };
        Self { size, inner }
    }
    pub fn downgrade(&self) -> LiveRendererSnapshotWeak {
        LiveRendererSnapshotWeak(std::sync::Arc::downgrade(&self.inner))
    }

    pub fn image_id(&self) -> LiveRendererImageId {
        LiveRendererImageId::from_raw(self.inner.image_id().raw())
    }
    pub const fn size(&self) -> sophia_protocol::Size {
        self.size
    }
    pub fn is_current(&self) -> bool {
        self.inner.is_current()
    }
    pub fn retire_import_cache(&self) {
        self.inner.retire_import_cache();
    }
}

impl LiveRendererImageSnapshot {
    pub fn retain(
        self,
        budget: &std::sync::Arc<super::LiveRendererSnapshotBudget>,
        epoch: std::sync::Arc<super::LiveRendererSnapshotEpoch>,
    ) -> Result<LiveRetainedRendererImageSnapshot, crate::LiveRendererScanoutBufferExportDetail>
    {
        let frame = self.inner.as_frame();
        let size = sophia_protocol::Size {
            width: i32::try_from(frame.width).map_err(|_| {
                crate::LiveRendererScanoutBufferExportDetail::InvalidBufferDescriptor
            })?,
            height: i32::try_from(frame.height).map_err(|_| {
                crate::LiveRendererScanoutBufferExportDetail::InvalidBufferDescriptor
            })?,
        };
        budget
            .retain(self.inner, epoch)
            .map(|inner| LiveRetainedRendererImageSnapshot { inner, size })
            .map_err(|error| super::reduced_native_owned_scanout_buffer_export_detail(error.into()))
    }
}

#[cfg(feature = "experimental-shell-capture")]
#[derive(Debug)]
#[must_use]
pub struct LiveRendererSnapshotRetentionFailure {
    pub detail: crate::LiveRendererScanoutBufferExportDetail,
    pub snapshot: LiveRendererImageSnapshot,
    pub reservation: super::LiveRendererSnapshotReservation,
}

#[cfg(feature = "experimental-shell-capture")]
impl LiveRendererImageSnapshot {
    /// Transfers reserved capacity, not proof of source-read completion.
    pub fn retain_reserved(
        self,
        budget: &std::sync::Arc<super::LiveRendererSnapshotBudget>,
        epoch: std::sync::Arc<super::LiveRendererSnapshotEpoch>,
        reservation: super::LiveRendererSnapshotReservation,
    ) -> Result<LiveRetainedRendererImageSnapshot, LiveRendererSnapshotRetentionFailure> {
        let frame = self.inner.as_frame();
        let size = i32::try_from(frame.width)
            .ok()
            .zip(i32::try_from(frame.height).ok())
            .filter(|(width, height)| *width > 0 && *height > 0)
            .map(|(width, height)| sophia_protocol::Size { width, height });
        let Some(size) = size else {
            return Err(LiveRendererSnapshotRetentionFailure {
                detail: crate::LiveRendererScanoutBufferExportDetail::InvalidBufferDescriptor,
                snapshot: self,
                reservation,
            });
        };
        let image_id = self.image_id;
        budget
            .retain_reserved(self.inner, epoch, reservation)
            .map(|inner| LiveRetainedRendererImageSnapshot { size, inner })
            .map_err(|failure| LiveRendererSnapshotRetentionFailure {
                detail: super::reduced_native_owned_scanout_buffer_export_detail(
                    failure.detail.into(),
                ),
                snapshot: Self {
                    image_id,
                    inner: failure.snapshot,
                },
                reservation: failure.reservation,
            })
    }
}

/// Promotion has already succeeded when snapshot export is refused. Callers
/// must settle the source Present independently of the optional preview copy.
#[derive(Debug)]
pub struct LiveRendererImagePromotion {
    pub promoted: bool,
    pub snapshot:
        Result<Option<LiveRendererImageSnapshot>, crate::LiveRendererScanoutBufferExportDetail>,
}
impl Default for LiveRendererImagePromotion {
    fn default() -> Self {
        Self {
            promoted: false,
            snapshot: Ok(None),
        }
    }
}
