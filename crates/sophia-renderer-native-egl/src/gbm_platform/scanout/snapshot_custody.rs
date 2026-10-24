//! Native error vocabulary around the shared descriptor/budget owner.
#[cfg(feature = "experimental-shell-capture")]
pub use sophia_renderer_image::{
    CaptureDestination as NativeCaptureDestination, CapturedImage as NativeCapturedImage,
    ManagedCaptureLease as NativeManagedCaptureLease,
    SnapshotReservation as NativeRendererSnapshotReservation,
    SnapshotRetentionFailure as NativeRendererSnapshotRetentionFailure,
};
pub use sophia_renderer_image::{
    SnapshotBudget as NativeRendererSnapshotBudget, SnapshotEpoch as NativeRendererSnapshotEpoch,
};
impl From<sophia_renderer_image::SnapshotError> for super::NativeGbmScanoutBufferExportDetail {
    fn from(error: sophia_renderer_image::SnapshotError) -> Self {
        use sophia_renderer_image::SnapshotError as E;
        match error {
            E::InvalidRendererImageId => Self::InvalidRendererImageId,
            E::InvalidBufferDescriptor => Self::InvalidBufferDescriptor,
            E::RendererImageStoreFull => Self::RendererImageStoreFull,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_storage_errors_preserve_native_refusal_classes() {
        use super::super::NativeGbmScanoutBufferExportDetail as Native;
        use sophia_renderer_image::SnapshotError as Shared;
        for (shared, native) in [
            (
                Shared::InvalidRendererImageId,
                Native::InvalidRendererImageId,
            ),
            (
                Shared::InvalidBufferDescriptor,
                Native::InvalidBufferDescriptor,
            ),
            (
                Shared::RendererImageStoreFull,
                Native::RendererImageStoreFull,
            ),
        ] {
            assert_eq!(Native::from(shared), native);
        }
    }
}
