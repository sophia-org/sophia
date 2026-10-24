//! Capture storage is not a source-completion receipt.
use crate::{RendererImageId, RendererImageSnapshot};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureImageMetadata {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub plane_count: u8,
}

#[derive(Debug)]
pub enum CapturedImage {
    Exported(Arc<RendererImageSnapshot>),
    #[cfg(feature = "experimental-shell-capture")]
    Managed(Arc<crate::ManagedCaptureLease>),
}

impl From<Arc<RendererImageSnapshot>> for CapturedImage {
    fn from(snapshot: Arc<RendererImageSnapshot>) -> Self {
        Self::Exported(snapshot)
    }
}

impl CapturedImage {
    pub fn metadata(&self) -> CaptureImageMetadata {
        match self {
            Self::Exported(image) => {
                let frame = image.as_frame();
                CaptureImageMetadata {
                    width: frame.width,
                    height: frame.height,
                    format: frame.format,
                    plane_count: frame.plane_count,
                }
            }
            #[cfg(feature = "experimental-shell-capture")]
            Self::Managed(lease) => CaptureImageMetadata {
                width: lease.info().width,
                height: lease.info().height,
                format: 0x3432_5258,
                plane_count: 1,
            },
        }
    }

    pub fn image_id(&self) -> RendererImageId {
        match self {
            Self::Exported(image) => image.image_id(),
            #[cfg(feature = "experimental-shell-capture")]
            Self::Managed(lease) => lease.info().image_id,
        }
    }

    pub fn is_current(&self) -> bool {
        match self {
            Self::Exported(image) => image.is_current(),
            #[cfg(feature = "experimental-shell-capture")]
            Self::Managed(lease) => lease.is_current(),
        }
    }

    pub fn is_retained(&self) -> bool {
        match self {
            Self::Exported(image) => image.is_retained(),
            #[cfg(feature = "experimental-shell-capture")]
            Self::Managed(_) => true,
        }
    }

    pub fn is_valid(&self) -> bool {
        match self {
            Self::Exported(image) => image.as_frame().is_valid(),
            #[cfg(feature = "experimental-shell-capture")]
            Self::Managed(lease) => lease.info().is_valid(),
        }
    }

    pub fn retire_import_cache(&self) {
        match self {
            Self::Exported(image) => image.retire_import_cache(),
            #[cfg(feature = "experimental-shell-capture")]
            Self::Managed(lease) => lease.retire(),
        }
    }

    /// Logical readers only. A managed allocation's physical charge lives in
    /// the renderer, independently of the resource owner's collection.
    pub fn custody_is_exclusive(image: &Arc<Self>) -> bool {
        Arc::strong_count(image) == 1
            && match &**image {
                Self::Exported(image) => RendererImageSnapshot::custody_is_exclusive(image),
                #[cfg(feature = "experimental-shell-capture")]
                Self::Managed(lease) => Arc::strong_count(lease) == 1,
            }
    }
}
