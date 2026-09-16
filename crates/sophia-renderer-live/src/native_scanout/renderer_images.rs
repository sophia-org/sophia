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
