//! Accepted storage, independent of renderer contexts and completion receipts.
use super::*;

#[cfg(feature = "experimental-shell-capture")]
pub use sophia_renderer_image::RendererImageSnapshot as ContentCapturedImage;

struct SealedPixels {
    description: ContentResourceBegin,
    bytes: CpuBytes,
}

// Comparator-only IPC adapter. Immutable owned bytes preserve the retired wire
// without conflating its release acknowledgement with the sealed image journal.
enum CpuBytes {
    Bytes(Vec<u8>),
    Sealed(sophia_linux_peer::SealedMapping),
}
impl CpuBytes {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Bytes(b) => b,
            Self::Sealed(m) => m.bytes(),
        }
    }
}

#[cfg(feature = "experimental-shell-capture")]
struct CapturedPixels {
    description: ContentResourceBegin,
    image: Arc<ContentCapturedImage>,
}

#[derive(Clone)]
enum Backing {
    Sealed(Arc<SealedPixels>),
    #[cfg(feature = "experimental-shell-capture")]
    Captured(Arc<CapturedPixels>),
}

/// Accepted storage. Each clone is a consumer; GPU imports additionally retain
/// the inner snapshot and its shared physical charge.
#[derive(Clone)]
pub struct ContentResourceLease(Backing);

/// A checked CPU view which retains the original accepted resource owner.
/// GPU backing cannot construct this type or reach its infallible byte view.
#[derive(Clone)]
pub struct ContentCpuResourceLease(Arc<SealedPixels>);

impl ContentCpuResourceLease {
    pub fn bytes(&self) -> &[u8] {
        self.0.bytes.bytes()
    }
    pub fn description(&self) -> &ContentResourceBegin {
        &self.0.description
    }
}

impl ContentResourceLease {
    pub(super) fn bytes_owned(description: ContentResourceBegin, bytes: Vec<u8>) -> Self {
        Self(Backing::Sealed(Arc::new(SealedPixels {
            description,
            bytes: CpuBytes::Bytes(bytes),
        })))
    }
    pub(super) fn needs_release_publication(&self) -> bool {
        match &self.0 {
            Backing::Sealed(p) => matches!(p.bytes, CpuBytes::Sealed(_)),
            #[cfg(feature = "experimental-shell-capture")]
            Backing::Captured(_) => true,
        }
    }
    pub(super) fn sealed(
        description: ContentResourceBegin,
        bytes: sophia_linux_peer::SealedMapping,
    ) -> Self {
        Self(Backing::Sealed(Arc::new(SealedPixels {
            description,
            bytes: CpuBytes::Sealed(bytes),
        })))
    }

    #[cfg(feature = "experimental-shell-capture")]
    pub(super) fn captured(
        description: ContentResourceBegin,
        image: Arc<ContentCapturedImage>,
    ) -> Self {
        Self(Backing::Captured(Arc::new(CapturedPixels {
            description,
            image,
        })))
    }

    pub fn cpu_bytes(&self) -> Option<&[u8]> {
        match &self.0 {
            Backing::Sealed(pixels) => Some(pixels.bytes.bytes()),
            #[cfg(feature = "experimental-shell-capture")]
            Backing::Captured(_) => None,
        }
    }

    pub fn into_cpu(self) -> Result<ContentCpuResourceLease, Self> {
        match self.0 {
            Backing::Sealed(pixels) => Ok(ContentCpuResourceLease(pixels)),
            #[cfg(feature = "experimental-shell-capture")]
            backing @ Backing::Captured(_) => Err(Self(backing)),
        }
    }

    pub fn description(&self) -> &ContentResourceBegin {
        match &self.0 {
            Backing::Sealed(pixels) => &pixels.description,
            #[cfg(feature = "experimental-shell-capture")]
            Backing::Captured(pixels) => &pixels.description,
        }
    }

    #[cfg(feature = "experimental-shell-capture")]
    pub fn captured_image(&self) -> Option<&Arc<ContentCapturedImage>> {
        match &self.0 {
            Backing::Sealed(_) => None,
            Backing::Captured(pixels) => Some(&pixels.image),
        }
    }

    pub(super) fn custody_is_exclusive(&self) -> bool {
        match &self.0 {
            Backing::Sealed(pixels) => Arc::strong_count(pixels) == 1,
            #[cfg(feature = "experimental-shell-capture")]
            Backing::Captured(pixels) => {
                Arc::strong_count(pixels) == 1 && pixels.image.custody_is_exclusive()
            }
        }
    }
}
