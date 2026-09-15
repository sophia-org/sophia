use std::sync::Arc;

/// Immutable bytes with their actual allocation owner. A shell source carries
/// the resource lease itself, not an independently cloned inner allocation.
/// Renderer-owned copies use Shared and are accounted by their backing owner.
#[derive(Clone)]
pub enum LiveCpuPixelStorage {
    Shared(Arc<Vec<u8>>),
    Content(sophia_runtime::ContentResourceLease),
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
