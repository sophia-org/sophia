use crate::prelude::*;
use std::{
    cell::Cell,
    os::fd::{BorrowedFd, OwnedFd},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Key {
    device_identity: u64,
    generation: u64,
    connector: drm::control::connector::Handle,
    crtc: drm::control::crtc::Handle,
    plane: drm::control::plane::Handle,
}

/// One successful ID lookup per physical head. This is owned by the native
/// head, never shared across devices, and discarded with the native owner on
/// seat release/replacement. Topology preparation explicitly invalidates it.
#[derive(Debug, Default)]
pub(crate) struct NativePrimaryPropertyCache {
    entry: Cell<Option<(Key, LibdrmNativePrimaryPlanePropertyHandles)>>,
}

impl NativePrimaryPropertyCache {
    pub(crate) fn invalidate(&self) {
        self.entry.set(None);
    }
}

/// Borrow the real device for one head visit. Only primary-plane ID discovery
/// is retained; all value reads, allocations, cleanup and commits reach it.
pub(crate) struct TargetPropertyDevice<'a, D> {
    device: &'a D,
    cache: &'a NativePrimaryPropertyCache,
    device_identity: u64,
    generation: u64,
}

impl<'a, D> TargetPropertyDevice<'a, D> {
    pub(crate) fn new(
        device: &'a D,
        cache: &'a NativePrimaryPropertyCache,
        device_identity: u64,
        generation: u64,
    ) -> Self {
        Self {
            device,
            cache,
            device_identity,
            generation,
        }
    }

    fn observed<T>(&self, result: io::Result<T>) -> io::Result<T> {
        // An I/O failure is not evidence that this target is still usable.
        // Preserve the original error and discover afresh on the next visit.
        if result.is_err() {
            self.cache.invalidate();
        }
        result
    }
}

impl<D: LibdrmNativePropertyLookupDevice> LibdrmNativePropertyLookupDevice
    for TargetPropertyDevice<'_, D>
{
    fn primary_plane_property_handles(
        &self,
        connector: drm::control::connector::Handle,
        crtc: drm::control::crtc::Handle,
        plane: drm::control::plane::Handle,
    ) -> LibdrmNativePrimaryPlanePropertyDiscoveryResult {
        let key = Key {
            device_identity: self.device_identity,
            generation: self.generation,
            connector,
            crtc,
            plane,
        };
        if let Some((previous, properties)) = self.cache.entry.get()
            && previous == key
        {
            return LibdrmNativePrimaryPlanePropertyDiscoveryResult {
                status: LibdrmNativePrimaryPlanePropertyDiscoveryStatus::Discovered,
                properties: Some(properties),
            };
        }
        // Never let a failed replacement leave an older target reusable.
        self.cache.invalidate();
        let result = self
            .device
            .primary_plane_property_handles(connector, crtc, plane);
        if result.status == LibdrmNativePrimaryPlanePropertyDiscoveryStatus::Discovered
            && let Some(properties) = result.properties
        {
            self.cache.entry.set(Some((key, properties)));
        }
        result
    }

    fn connector_property_handles(
        &self,
        h: drm::control::connector::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.observed(self.device.connector_property_handles(h))
    }
    fn crtc_property_handles(
        &self,
        h: drm::control::crtc::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.observed(self.device.crtc_property_handles(h))
    }
    fn plane_property_handles(
        &self,
        h: drm::control::plane::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.observed(self.device.plane_property_handles(h))
    }
    fn connector_property_value(
        &self,
        h: drm::control::connector::Handle,
        p: drm::control::property::Handle,
    ) -> io::Result<Option<u64>> {
        self.observed(self.device.connector_property_value(h, p))
    }
}

impl<D: LibdrmNativeKmsSelectionDevice> LibdrmNativeKmsSelectionDevice
    for TargetPropertyDevice<'_, D>
{
    fn connector_handles(&self) -> io::Result<Vec<drm::control::connector::Handle>> {
        self.observed(self.device.connector_handles())
    }
    fn crtc_handles(&self) -> io::Result<Vec<drm::control::crtc::Handle>> {
        self.observed(self.device.crtc_handles())
    }
    fn connector_snapshot(
        &self,
        h: drm::control::connector::Handle,
    ) -> io::Result<LibdrmNativeConnectorSnapshot> {
        self.observed(self.device.connector_snapshot(h))
    }
    fn encoder_snapshot(
        &self,
        h: drm::control::encoder::Handle,
    ) -> io::Result<LibdrmNativeEncoderSnapshot> {
        self.observed(self.device.encoder_snapshot(h))
    }
    fn plane_handles(&self) -> io::Result<Vec<drm::control::plane::Handle>> {
        self.observed(self.device.plane_handles())
    }
    fn plane_snapshot(
        &self,
        h: drm::control::plane::Handle,
    ) -> io::Result<LibdrmNativePlaneSnapshot> {
        self.observed(self.device.plane_snapshot(h))
    }
    fn plane_type(
        &self,
        h: drm::control::plane::Handle,
    ) -> io::Result<Option<drm::control::PlaneType>> {
        self.observed(self.device.plane_type(h))
    }
}

impl<D: LibdrmNativePrimaryPlaneResourceDevice> LibdrmNativePrimaryPlaneResourceDevice
    for TargetPropertyDevice<'_, D>
{
    fn create_mode_blob_for_selection(
        &self,
        s: LibdrmNativePrimaryPlaneSelection,
    ) -> io::Result<u64> {
        self.observed(self.device.create_mode_blob_for_selection(s))
    }
    fn create_mode_blob(&self, m: drm::control::Mode) -> io::Result<u64> {
        self.observed(self.device.create_mode_blob(m))
    }
    fn add_scanout_framebuffer_with_modifiers<B: drm::buffer::PlanarBuffer + ?Sized>(
        &self,
        b: &B,
    ) -> io::Result<drm::control::framebuffer::Handle> {
        self.observed(self.device.add_scanout_framebuffer_with_modifiers(b))
    }
    fn add_scanout_framebuffer_without_modifiers<B: drm::buffer::PlanarBuffer + ?Sized>(
        &self,
        b: &B,
    ) -> io::Result<drm::control::framebuffer::Handle> {
        self.observed(self.device.add_scanout_framebuffer_without_modifiers(b))
    }
    fn add_legacy_scanout_framebuffer<B: drm::buffer::Buffer + ?Sized>(
        &self,
        b: &B,
        depth: u32,
        bpp: u32,
    ) -> io::Result<drm::control::framebuffer::Handle> {
        self.observed(self.device.add_legacy_scanout_framebuffer(b, depth, bpp))
    }
    fn destroy_scanout_framebuffer(&self, h: drm::control::framebuffer::Handle) -> io::Result<()> {
        self.observed(self.device.destroy_scanout_framebuffer(h))
    }
    fn import_scanout_dma_buf(&self, fd: BorrowedFd<'_>) -> io::Result<drm::buffer::Handle> {
        self.observed(self.device.import_scanout_dma_buf(fd))
    }
    fn close_scanout_buffer(&self, h: drm::buffer::Handle) -> io::Result<()> {
        self.observed(self.device.close_scanout_buffer(h))
    }
    fn destroy_mode_blob(&self, h: u64) -> io::Result<()> {
        self.observed(self.device.destroy_mode_blob(h))
    }
}

impl<D: LibdrmNativeAtomicCommitDevice> LibdrmNativeAtomicCommitDevice
    for TargetPropertyDevice<'_, D>
{
    fn submit_atomic_commit(
        &self,
        flags: drm::control::AtomicCommitFlags,
        request: drm::control::atomic::AtomicModeReq,
    ) -> io::Result<()> {
        self.observed(self.device.submit_atomic_commit(flags, request))
    }
    fn submit_atomic_commit_with_out_fence(
        &self,
        flags: drm::control::AtomicCommitFlags,
        request: drm::control::atomic::AtomicModeReq,
        crtc: drm::control::crtc::Handle,
        property: drm::control::property::Handle,
    ) -> io::Result<Option<OwnedFd>> {
        self.observed(
            self.device
                .submit_atomic_commit_with_out_fence(flags, request, crtc, property),
        )
    }
}

#[cfg(test)]
#[path = "../../../../tests/support/native_property_cache.rs"]
mod tests;
