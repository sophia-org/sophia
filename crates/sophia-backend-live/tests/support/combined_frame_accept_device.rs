//! G3-4: the one deterministic device boundary of the accept fixture. It is
//! the composition smoke's device with counts of the framebuffers and
//! commits the real preparation and tracked submission ask it for, and a
//! switch to refuse either.

use crate::{
    DeterministicPrimaryPlaneScanoutDevice as Inner, LibdrmNativeAtomicCommitDevice,
    LibdrmNativeConnectorSnapshot, LibdrmNativeEncoderSnapshot, LibdrmNativeKmsSelectionDevice,
    LibdrmNativePlaneSnapshot, LibdrmNativePrimaryPlaneResourceDevice,
    LibdrmNativePrimaryPlaneSelection, LibdrmNativePropertyHandleSet,
    LibdrmNativePropertyLookupDevice,
};
use std::cell::Cell;
use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// Under test-support alone the running fixture refuses nothing (G3-5d d-1).
#[cfg_attr(not(test), allow(dead_code))]
pub(super) enum Refuse {
    Nothing,
    Framebuffer,
    Commit,
}

#[derive(Debug)]
pub(super) struct CountingDevice {
    inner: Inner,
    refuse: Refuse,
    framebuffers: Cell<usize>,
    /// Commits without `TEST_ONLY`.
    commits: Cell<usize>,
    test_commits: Cell<usize>,
}

impl CountingDevice {
    pub(super) fn new(refuse: Refuse) -> Self {
        Self {
            inner: Inner::ready(),
            refuse,
            framebuffers: Cell::new(0),
            commits: Cell::new(0),
            test_commits: Cell::new(0),
        }
    }

    /// Framebuffers asked for, commits, and validating commits.
    pub(super) fn counts(&self) -> (usize, usize, usize) {
        (
            self.framebuffers.get(),
            self.commits.get(),
            self.test_commits.get(),
        )
    }

    fn framebuffer(&self) -> io::Result<()> {
        self.framebuffers.set(self.framebuffers.get() + 1);
        if self.refuse == Refuse::Framebuffer {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    }
}

impl LibdrmNativeKmsSelectionDevice for CountingDevice {
    fn connector_handles(&self) -> io::Result<Vec<::drm::control::connector::Handle>> {
        self.inner.connector_handles()
    }

    fn crtc_handles(&self) -> io::Result<Vec<::drm::control::crtc::Handle>> {
        self.inner.crtc_handles()
    }

    fn connector_snapshot(
        &self,
        connector: ::drm::control::connector::Handle,
    ) -> io::Result<LibdrmNativeConnectorSnapshot> {
        self.inner.connector_snapshot(connector)
    }

    fn encoder_snapshot(
        &self,
        encoder: ::drm::control::encoder::Handle,
    ) -> io::Result<LibdrmNativeEncoderSnapshot> {
        self.inner.encoder_snapshot(encoder)
    }

    fn plane_handles(&self) -> io::Result<Vec<::drm::control::plane::Handle>> {
        self.inner.plane_handles()
    }

    fn plane_snapshot(
        &self,
        plane: ::drm::control::plane::Handle,
    ) -> io::Result<LibdrmNativePlaneSnapshot> {
        self.inner.plane_snapshot(plane)
    }

    fn plane_type(
        &self,
        plane: ::drm::control::plane::Handle,
    ) -> io::Result<Option<::drm::control::PlaneType>> {
        self.inner.plane_type(plane)
    }
}

impl LibdrmNativePropertyLookupDevice for CountingDevice {
    fn connector_property_handles(
        &self,
        connector: ::drm::control::connector::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.inner.connector_property_handles(connector)
    }

    fn crtc_property_handles(
        &self,
        crtc: ::drm::control::crtc::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.inner.crtc_property_handles(crtc)
    }

    fn plane_property_handles(
        &self,
        plane: ::drm::control::plane::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.inner.plane_property_handles(plane)
    }
}

impl LibdrmNativePrimaryPlaneResourceDevice for CountingDevice {
    fn create_mode_blob_for_selection(
        &self,
        selection: LibdrmNativePrimaryPlaneSelection,
    ) -> io::Result<u64> {
        self.inner.create_mode_blob_for_selection(selection)
    }

    fn create_mode_blob(&self, mode: ::drm::control::Mode) -> io::Result<u64> {
        self.inner.create_mode_blob(mode)
    }

    fn add_scanout_framebuffer_with_modifiers<B>(
        &self,
        buffer: &B,
    ) -> io::Result<::drm::control::framebuffer::Handle>
    where
        B: ::drm::buffer::PlanarBuffer + ?Sized,
    {
        self.framebuffer()?;
        self.inner.add_scanout_framebuffer_with_modifiers(buffer)
    }

    fn add_scanout_framebuffer_without_modifiers<B>(
        &self,
        buffer: &B,
    ) -> io::Result<::drm::control::framebuffer::Handle>
    where
        B: ::drm::buffer::PlanarBuffer + ?Sized,
    {
        self.framebuffer()?;
        self.inner.add_scanout_framebuffer_without_modifiers(buffer)
    }

    fn add_legacy_scanout_framebuffer<B>(
        &self,
        buffer: &B,
        depth: u32,
        bpp: u32,
    ) -> io::Result<::drm::control::framebuffer::Handle>
    where
        B: ::drm::buffer::Buffer + ?Sized,
    {
        self.framebuffer()?;
        self.inner
            .add_legacy_scanout_framebuffer(buffer, depth, bpp)
    }

    fn destroy_scanout_framebuffer(
        &self,
        framebuffer: ::drm::control::framebuffer::Handle,
    ) -> io::Result<()> {
        self.inner.destroy_scanout_framebuffer(framebuffer)
    }

    fn destroy_mode_blob(&self, mode_blob: u64) -> io::Result<()> {
        self.inner.destroy_mode_blob(mode_blob)
    }
}

impl LibdrmNativeAtomicCommitDevice for CountingDevice {
    fn submit_atomic_commit(
        &self,
        flags: ::drm::control::AtomicCommitFlags,
        request: ::drm::control::atomic::AtomicModeReq,
    ) -> io::Result<()> {
        if flags.contains(::drm::control::AtomicCommitFlags::TEST_ONLY) {
            self.test_commits.set(self.test_commits.get() + 1);
        } else {
            self.commits.set(self.commits.get() + 1);
        }
        if self.refuse == Refuse::Commit {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.inner.submit_atomic_commit(flags, request)
    }
}
