mod config;
mod dmabuf_capabilities;
pub use dmabuf_capabilities::*;
mod pixmap_probe;
pub use pixmap_probe::*;
mod repaint;
pub use repaint::*;
mod scanout;
mod smoke;

pub use scanout::*;
pub use smoke::*;

const EGL_PLATFORM_GBM_KHR: khronos_egl::Enum = 0x31D7;
/// `EGL_EXT_buffer_age`. khronos-egl 6.0.0 binds `eglQuerySurface` but defines
/// no attribute for it, so the value is declared here beside the platform enum.
const EGL_BUFFER_AGE_EXT: khronos_egl::Int = 0x313D;
const EGL_EXT_BUFFER_AGE_NAME: &str = "EGL_EXT_buffer_age";
