//! Descriptor and budget ownership shared by runtime leases and the native renderer.
//!
//! This crate contains no rendering context, device, capture executor, or completion
//! receipts. An owned or retained snapshot is storage, not proof of immutability or
//! ended source reads. Admission must still consume the renderer's opaque receipt.

mod captured;
mod custody;
#[cfg(feature = "experimental-shell-capture")]
mod managed;
#[cfg(feature = "experimental-shell-capture")]
mod prepared;
mod snapshot;
pub use captured::*;
pub use custody::*;
#[cfg(feature = "experimental-shell-capture")]
pub use managed::*;
#[cfg(feature = "experimental-shell-capture")]
pub use prepared::*;
pub use snapshot::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    InvalidRendererImageId,
    InvalidBufferDescriptor,
    RendererImageStoreFull,
}
impl core::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SnapshotError {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RendererImageId(u64);

impl RendererImageId {
    pub const INVALID: Self = Self(0);

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}
