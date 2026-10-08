#[cfg(all(test, feature = "libdrm-events"))]
#[path = "../../tests/support/framebuffer_device.rs"]
pub(crate) mod framebuffer_test_device;
#[cfg(feature = "libdrm-events")]
mod native_atomic;
#[cfg(feature = "libdrm-events")]
mod native_kms;
#[cfg(feature = "libdrm-events")]
mod native_page_flip;
#[cfg(feature = "libdrm-events")]
mod native_primary_plane;
#[cfg(feature = "libdrm-events")]
mod native_scanout;
#[cfg(feature = "drm-hotplug")]
mod render_inventory;
#[cfg(any(
    feature = "drm-hotplug",
    all(feature = "seat-control", feature = "libdrm-events")
))]
pub(crate) mod seat_inventory;
mod sysfs_discovery;
#[cfg(feature = "drm-hotplug")]
mod topology_monitor;

#[cfg(feature = "libdrm-events")]
pub use native_atomic::*;
#[cfg(feature = "libdrm-events")]
pub use native_kms::*;
#[cfg(feature = "libdrm-events")]
pub use native_page_flip::*;
#[cfg(feature = "libdrm-events")]
pub use native_primary_plane::*;
#[cfg(feature = "libdrm-events")]
pub use native_scanout::*;
#[cfg(feature = "drm-hotplug")]
pub use render_inventory::*;
pub use sysfs_discovery::*;
#[cfg(feature = "drm-hotplug")]
pub use topology_monitor::*;
