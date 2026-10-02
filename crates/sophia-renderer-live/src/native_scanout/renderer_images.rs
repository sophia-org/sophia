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
}

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
            .map_err(super::reduced_native_owned_scanout_buffer_export_detail)
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
