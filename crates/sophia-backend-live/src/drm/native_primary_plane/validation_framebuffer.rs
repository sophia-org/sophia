use crate::prelude::*;

/// A KMS framebuffer handle, named for crates without a DRM dependency.
#[cfg(feature = "libdrm-events")]
pub type LibdrmNativeFramebufferHandle = drm::control::framebuffer::Handle;

/// The four calls a validation framebuffer needs from a card.
///
/// A complete topology test names a framebuffer on every enabled CRTC, because
/// drivers such as amdgpu refuse to enable a CRTC whose primary plane is off
/// (`amdgpu_dm_crtc.c`, "Can't enable a CRTC without enabling the primary
/// plane"). The buffer is allocated for the test alone, so the answer never
/// depends on what a previous owner left bound to the plane.
#[cfg(feature = "libdrm-events")]
pub trait LibdrmNativeValidationBufferDevice {
    type Buffer;

    fn create_validation_buffer(&self, width: u32, height: u32) -> io::Result<Self::Buffer>;

    fn add_validation_framebuffer(
        &self,
        buffer: &Self::Buffer,
    ) -> io::Result<drm::control::framebuffer::Handle>;

    fn destroy_validation_framebuffer(
        &self,
        framebuffer: drm::control::framebuffer::Handle,
    ) -> io::Result<()>;

    fn destroy_validation_buffer(&self, buffer: Self::Buffer) -> io::Result<()>;
}

#[cfg(feature = "libdrm-events")]
impl<D> LibdrmNativeValidationBufferDevice for D
where
    D: drm::control::Device,
{
    type Buffer = drm::control::dumbbuffer::DumbBuffer;

    fn create_validation_buffer(&self, width: u32, height: u32) -> io::Result<Self::Buffer> {
        self.create_dumb_buffer((width, height), drm::buffer::DrmFourcc::Xrgb8888, 32)
    }

    fn add_validation_framebuffer(
        &self,
        buffer: &Self::Buffer,
    ) -> io::Result<drm::control::framebuffer::Handle> {
        self.add_framebuffer(buffer, 24, 32)
    }

    fn destroy_validation_framebuffer(
        &self,
        framebuffer: drm::control::framebuffer::Handle,
    ) -> io::Result<()> {
        self.destroy_framebuffer(framebuffer)
    }

    fn destroy_validation_buffer(&self, buffer: Self::Buffer) -> io::Result<()> {
        self.destroy_dumb_buffer(buffer)
    }
}

/// A buffer and its framebuffer, owned by one topology test on one card.
///
/// It is released on the card that allocated it and nowhere else, which is why
/// it carries no card: the owner that knows the card releases it. A test that
/// fails, and a resolution that fails partway, release theirs the same way.
#[cfg(feature = "libdrm-events")]
#[derive(Debug)]
pub struct LibdrmNativeValidationFramebuffer<B> {
    buffer: B,
    framebuffer: drm::control::framebuffer::Handle,
    size: Size,
}

#[cfg(feature = "libdrm-events")]
impl<B> LibdrmNativeValidationFramebuffer<B> {
    /// Allocates all or nothing: a framebuffer that cannot be added leaves no
    /// buffer behind.
    pub fn allocate<D>(card: &D, width: u32, height: u32) -> io::Result<Self>
    where
        D: LibdrmNativeValidationBufferDevice<Buffer = B>,
    {
        let size = Size {
            width: i32::try_from(width).map_err(|_| oversized())?,
            height: i32::try_from(height).map_err(|_| oversized())?,
        };
        let buffer = card.create_validation_buffer(width, height)?;
        match card.add_validation_framebuffer(&buffer) {
            Ok(framebuffer) => Ok(Self {
                buffer,
                framebuffer,
                size,
            }),
            Err(error) => {
                let _ = card.destroy_validation_buffer(buffer);
                Err(error)
            }
        }
    }

    pub const fn framebuffer(&self) -> drm::control::framebuffer::Handle {
        self.framebuffer
    }

    pub const fn size(&self) -> Size {
        self.size
    }

    /// Removes the framebuffer, then the buffer, attempting both, and reports the
    /// first failure.
    pub fn release<D>(self, card: &D) -> io::Result<()>
    where
        D: LibdrmNativeValidationBufferDevice<Buffer = B>,
    {
        let framebuffer = card.destroy_validation_framebuffer(self.framebuffer);
        let buffer = card.destroy_validation_buffer(self.buffer);
        framebuffer.and(buffer)
    }
}

#[cfg(feature = "libdrm-events")]
fn oversized() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "validation framebuffer size exceeds the scanout coordinate range",
    )
}
