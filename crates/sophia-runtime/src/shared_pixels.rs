//! Immutable CPU pixels whose backing follows every renderer lease.
use std::ops::Deref;
use std::sync::Arc;

#[derive(Clone)]
enum Backing {
    Bytes(Arc<[u8]>),
    Sealed(Arc<sophia_linux_peer::SealedMapping>),
}

/// Cloning retains backing without copying pixels. A sealed backing can only
/// enter through a completed `ValidatedSealedPixels` scan; the mapping remains
/// alive until the last consumer drops its handle.
#[derive(Clone)]
pub struct SharedPixels(Backing);

impl From<Arc<[u8]>> for SharedPixels {
    fn from(bytes: Arc<[u8]>) -> Self {
        Self(Backing::Bytes(bytes))
    }
}

impl SharedPixels {
    pub(crate) fn sealed(mapping: sophia_linux_peer::SealedMapping) -> Self {
        Self(Backing::Sealed(Arc::new(mapping)))
    }
}

impl Deref for SharedPixels {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match &self.0 {
            Backing::Bytes(bytes) => bytes,
            Backing::Sealed(mapping) => mapping.bytes(),
        }
    }
}

impl AsRef<[u8]> for SharedPixels {
    fn as_ref(&self) -> &[u8] {
        self
    }
}
