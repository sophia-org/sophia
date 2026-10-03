use std::sync::Arc;

/// Immutable bytes with their actual allocation owner. A shell source carries
/// the resource lease itself, not an independently cloned inner allocation.
/// Renderer-owned copies use Shared and are accounted by their backing owner.
#[derive(Clone)]
pub enum LiveCpuPixelStorage {
    Shared(Arc<Vec<u8>>),
    Content(sophia_runtime::ContentResourceLease),
    /// A lock provider's image, shared with Session's custody.
    Bytes(Arc<[u8]>),
}

impl LiveCpuPixelStorage {
    pub fn as_slice(&self) -> &[u8] {
        self
    }
}

impl From<Arc<Vec<u8>>> for LiveCpuPixelStorage {
    fn from(bytes: Arc<Vec<u8>>) -> Self {
        Self::Shared(bytes)
    }
}

impl From<sophia_engine::CompositorImageSource> for LiveCpuPixelStorage {
    fn from(source: sophia_engine::CompositorImageSource) -> Self {
        match source {
            sophia_engine::CompositorImageSource::Shell(lease) => Self::Content(lease),
            sophia_engine::CompositorImageSource::Lock(image) => Self::Bytes(image.pixels),
        }
    }
}

impl From<sophia_runtime::ContentResourceLease> for LiveCpuPixelStorage {
    fn from(lease: sophia_runtime::ContentResourceLease) -> Self {
        Self::Content(lease)
    }
}

impl core::ops::Deref for LiveCpuPixelStorage {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Shared(bytes) => bytes,
            Self::Content(lease) => lease.bytes(),
            Self::Bytes(bytes) => bytes,
        }
    }
}

impl core::fmt::Debug for LiveCpuPixelStorage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LiveCpuPixelStorage")
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

impl PartialEq for LiveCpuPixelStorage {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
impl Eq for LiveCpuPixelStorage {}

/// The texture handle of a lock provider image. Lock images take the space
/// with both top bits set, apart from surface buffers and the shell's
/// handles; every part of the identity moves it, so a replaced provider never
/// reuses a texture.
pub fn lock_image_handle(identity: sophia_engine::SessionLockImageIdentity) -> u64 {
    let mixed = identity.output.raw().rotate_left(48)
        ^ identity.connection_epoch.rotate_left(32)
        ^ identity.resource_id.rotate_left(16)
        ^ identity.resource_generation;
    (0b11 << 62) | (mixed & ((1 << 62) - 1))
}
